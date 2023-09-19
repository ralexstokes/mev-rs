use async_trait::async_trait;
use beacon_api_client::Client as ApiClient;
use ethereum_consensus::{
    bellatrix::mainnet as bellatrix,
    builder::{SignedValidatorRegistration, ValidatorRegistration},
    capella::mainnet as capella,
    crypto::SecretKey,
    phase0::mainnet::{compute_domain, Validator},
    primitives::{BlsPublicKey, DomainType, ExecutionAddress, Hash32, Root, Slot, U256},
    signing::sign_with_domain,
    state_transition::{Context, Forks},
};
use mev_boost_rs::{Config, Service};
use mev_rs::{
    blinded_block_provider::{BlindedBlockProvider, Client as RelayClient, Server as RelayServer},
    signing::sign_builder_message,
    types::{
        bellatrix as bellatrix_builder, capella as capella_builder, BidRequest, ExecutionPayload,
        SignedBlindedBeaconBlock, SignedBuilderBid,
    },
    Error,
};
use rand::seq::SliceRandom;
use std::{
    collections::HashMap,
    net::Ipv4Addr,
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

fn setup_logging() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "error".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn get_time() -> u64 {
    let duration = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    duration.as_secs()
}

struct Proposer {
    index: usize,
    validator: Validator,
    signing_key: SecretKey,
    fee_recipient: ExecutionAddress,
}

fn create_proposers<R: rand::Rng>(rng: &mut R, count: usize) -> Vec<Proposer> {
    (0..count)
        .map(|i| {
            let signing_key = SecretKey::random(rng).unwrap();
            let public_key = signing_key.public_key();

            let validator = Validator { public_key, ..Default::default() };

            let fee_recipient = ExecutionAddress::try_from([i as u8; 20].as_ref()).unwrap();

            Proposer { index: i, validator, signing_key, fee_recipient }
        })
        .collect()
}

#[derive(Default, Clone)]
pub struct IdentityBuilder {
    signing_key: SecretKey,
    public_key: BlsPublicKey,
    context: Arc<Context>,
    bids: Arc<Mutex<HashMap<Slot, ExecutionPayload>>>,
    registrations: Arc<Mutex<HashMap<BlsPublicKey, ValidatorRegistration>>>,
}

impl IdentityBuilder {
    fn new(context: Context) -> Self {
        let signing_key = SecretKey::try_from([1u8; 32].as_ref()).unwrap();
        let public_key = signing_key.public_key();
        Self { signing_key, public_key, context: Arc::new(context), ..Default::default() }
    }
}

#[async_trait]
impl BlindedBlockProvider for IdentityBuilder {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let mut state = self.registrations.lock().unwrap();
        for registration in registrations {
            let registration = &registration.message;
            let public_key = registration.public_key.clone();
            state.insert(public_key, registration.clone());
        }
        Ok(())
    }

    async fn fetch_best_bid(
        &self,
        BidRequest { slot, parent_hash, public_key }: &BidRequest,
    ) -> Result<SignedBuilderBid, Error> {
        let capella_fork_slot = self.context.capella_fork_epoch * self.context.slots_per_epoch;
        let state = self.registrations.lock().unwrap();
        let preferences = state.get(public_key).unwrap();
        let value = U256::from(1337);
        let (payload, signed_builder_bid) = if *slot < capella_fork_slot {
            let mut inner = bellatrix::ExecutionPayload {
                parent_hash: parent_hash.clone(),
                fee_recipient: preferences.fee_recipient.clone(),
                gas_limit: preferences.gas_limit,
                ..Default::default()
            };
            let header = bellatrix::ExecutionPayloadHeader::try_from(&mut inner).unwrap();
            let payload = ExecutionPayload::Bellatrix(inner);
            let mut inner = bellatrix_builder::BuilderBid {
                header,
                value,
                public_key: self.public_key.clone(),
            };
            let signature =
                sign_builder_message(&mut inner, &self.signing_key, &self.context).unwrap();
            let inner = bellatrix_builder::SignedBuilderBid { message: inner, signature };
            (payload, SignedBuilderBid::Bellatrix(inner))
        } else {
            let mut inner = capella::ExecutionPayload {
                parent_hash: parent_hash.clone(),
                fee_recipient: preferences.fee_recipient.clone(),
                gas_limit: preferences.gas_limit,
                ..Default::default()
            };
            let header = capella::ExecutionPayloadHeader::try_from(&mut inner).unwrap();
            let payload = ExecutionPayload::Capella(inner);
            let mut inner =
                capella_builder::BuilderBid { header, value, public_key: self.public_key.clone() };
            let signature =
                sign_builder_message(&mut inner, &self.signing_key, &self.context).unwrap();
            let inner = capella_builder::SignedBuilderBid { message: inner, signature };
            (payload, SignedBuilderBid::Capella(inner))
        };

        let mut state = self.bids.lock().unwrap();
        state.insert(*slot, payload);
        Ok(signed_builder_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let slot = signed_block.slot();
        let state = self.bids.lock().unwrap();
        Ok(state.get(&slot).cloned().unwrap())
    }
}

#[tokio::test]
async fn test_end_to_end() {
    setup_logging();

    let mut rng = rand::thread_rng();

    let mut proposers = create_proposers(&mut rng, 4);

    let genesis_validators_root = Root::try_from([23u8; 32].as_ref()).unwrap();

    let mut context = Context::for_mainnet();
    // mock epoch values to transition across forks
    context.bellatrix_fork_epoch = 12;
    context.capella_fork_epoch = 22;

    // NOTE: non-default secret key required. otherwise public key is point at infinity and
    // signature verification will fail.
    let key_bytes: &[u8] = &[1u8; 32];
    let secret_key = SecretKey::try_from(key_bytes).unwrap();
    let relay_public_key = secret_key.public_key();

    let host = Ipv4Addr::LOCALHOST;
    let port = 28545;
    let builder = IdentityBuilder::new(context.clone());
    let relay = RelayServer::new(host, port, builder);
    std::mem::drop(relay.spawn());

    // start mux server
    let mut config = Config::default();
    config.relays.push(format!("http://{relay_public_key}@127.0.0.1:{port}"));

    let mux_port = config.port;
    let service = Service::from(config);
    service.spawn(Some(context.clone())).unwrap();

    let beacon_node = RelayClient::new(ApiClient::new(
        Url::parse(&format!("http://127.0.0.1:{mux_port}")).unwrap(),
    ));

    beacon_node.check_status().await.unwrap();

    let registrations = proposers
        .iter()
        .map(|proposer| {
            let timestamp = get_time();
            let mut registration = ValidatorRegistration {
                fee_recipient: proposer.fee_recipient.clone(),
                gas_limit: 30_000_000,
                timestamp,
                public_key: proposer.validator.public_key.clone(),
            };
            let signature =
                sign_builder_message(&mut registration, &proposer.signing_key, &context).unwrap();
            SignedValidatorRegistration { message: registration, signature }
        })
        .collect::<Vec<_>>();
    beacon_node.register_validators(&registrations).await.unwrap();

    beacon_node.check_status().await.unwrap();

    proposers.shuffle(&mut rng);

    for (i, proposer) in proposers.iter().enumerate() {
        propose_block(&beacon_node, proposer, i, &context, &genesis_validators_root).await;
    }
}

async fn propose_block(
    beacon_node: &RelayClient,
    proposer: &Proposer,
    shuffling_index: usize,
    context: &Context,
    genesis_validators_root: &Root,
) {
    let fork = if shuffling_index == 0 { Forks::Bellatrix } else { Forks::Capella };
    let current_slot = match fork {
        Forks::Bellatrix => 32 + context.bellatrix_fork_epoch * context.slots_per_epoch,
        Forks::Capella => 32 + context.capella_fork_epoch * context.slots_per_epoch,
        _ => unimplemented!(),
    };
    let parent_hash = Hash32::try_from([shuffling_index as u8; 32].as_ref()).unwrap();

    let request = BidRequest {
        slot: current_slot,
        parent_hash: parent_hash.clone(),
        public_key: proposer.validator.public_key.clone(),
    };
    let signed_bid = beacon_node.fetch_best_bid(&request).await.unwrap();
    let bid_parent_hash = signed_bid.parent_hash();
    assert_eq!(bid_parent_hash, &parent_hash);

    let signed_block = match fork {
        Forks::Bellatrix => {
            let bid = match signed_bid {
                SignedBuilderBid::Bellatrix(bid) => bid,
                _ => unimplemented!(),
            };
            let beacon_block_body = bellatrix::BlindedBeaconBlockBody {
                execution_payload_header: bid.message.header,
                ..Default::default()
            };
            let mut beacon_block = bellatrix::BlindedBeaconBlock {
                slot: current_slot,
                proposer_index: proposer.index,
                body: beacon_block_body,
                ..Default::default()
            };
            let fork_version = context.bellatrix_fork_version;
            let domain = compute_domain(
                DomainType::BeaconProposer,
                Some(fork_version),
                Some(*genesis_validators_root),
                context,
            )
            .unwrap();
            let signature =
                sign_with_domain(&mut beacon_block, &proposer.signing_key, domain).unwrap();
            let signed_block =
                bellatrix::SignedBlindedBeaconBlock { message: beacon_block, signature };
            SignedBlindedBeaconBlock::Bellatrix(signed_block)
        }
        Forks::Capella => {
            let bid = match signed_bid {
                SignedBuilderBid::Capella(bid) => bid,
                _ => unimplemented!(),
            };
            let beacon_block_body = capella::BlindedBeaconBlockBody {
                execution_payload_header: bid.message.header,
                ..Default::default()
            };
            let mut beacon_block = capella::BlindedBeaconBlock {
                slot: current_slot,
                proposer_index: proposer.index,
                body: beacon_block_body,
                ..Default::default()
            };
            let fork_version = context.capella_fork_version;
            let domain = compute_domain(
                DomainType::BeaconProposer,
                Some(fork_version),
                Some(*genesis_validators_root),
                context,
            )
            .unwrap();
            let signature =
                sign_with_domain(&mut beacon_block, &proposer.signing_key, domain).unwrap();
            let signed_block =
                capella::SignedBlindedBeaconBlock { message: beacon_block, signature };
            SignedBlindedBeaconBlock::Capella(signed_block)
        }
        _ => unimplemented!(),
    };

    beacon_node.check_status().await.unwrap();

    let payload = beacon_node.open_bid(&signed_block).await.unwrap();

    match payload {
        ExecutionPayload::Bellatrix(payload) => {
            assert_eq!(payload.parent_hash, parent_hash);
            assert_eq!(payload.fee_recipient, proposer.fee_recipient);
        }
        ExecutionPayload::Capella(payload) => {
            assert_eq!(payload.parent_hash, parent_hash);
            assert_eq!(payload.fee_recipient, proposer.fee_recipient);
        }
        _ => unimplemented!(),
    }

    beacon_node.check_status().await.unwrap();
}
