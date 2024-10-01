mod identity_builder;

use beacon_api_client::Client as ApiClient;
use ethereum_consensus::{
    builder::{SignedValidatorRegistration, ValidatorRegistration},
    crypto::SecretKey,
    networks::Network,
    phase0::{compute_domain, Validator},
    primitives::{DomainType, ExecutionAddress, Hash32, Root},
    signing::sign_with_domain,
    state_transition::Context,
    Fork,
};
use identity_builder::*;
use mev_boost_rs::{Config, Service};
use mev_rs::{
    blinded_block_provider::{Client as RelayClient, Server as RelayServer},
    signing::sign_builder_message,
    types::{AuctionRequest, SignedBlindedBeaconBlock},
};
use rand::seq::SliceRandom;
use std::{
    net::Ipv4Addr,
    time::{SystemTime, UNIX_EPOCH},
};
use url::Url;

#[cfg(not(feature = "minimal-preset"))]
use ethereum_consensus::{bellatrix::mainnet as bellatrix, capella::mainnet as capella};
#[cfg(feature = "minimal-preset")]
use ethereum_consensus::{bellatrix::minimal as bellatrix, capella::minimal as capella};

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

// TODO: Remove once this is fixed between clippy and tokio
// https://github.com/rust-lang/rust-clippy/pull/13464
#[allow(clippy::needless_return)]
#[tokio::test]
async fn test_end_to_end() {
    setup_logging();

    let mut rng = rand::thread_rng();

    let mut proposers = create_proposers(&mut rng, 4);

    let genesis_validators_root = Root::try_from([23u8; 32].as_ref()).unwrap();

    let network = Network::Sepolia;
    let context = Context::try_from(network.clone()).unwrap();

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
    config.relays.push(format!("http://{relay_public_key:?}@127.0.0.1:{port}"));

    let mux_port = config.port;
    let service = Service::from(network, config);
    service.spawn().unwrap();

    let beacon_node = RelayClient::new(ApiClient::new(
        Url::parse(&format!("http://127.0.0.1:{mux_port}")).unwrap(),
    ));

    beacon_node.check_status().await.unwrap();

    let registrations = proposers
        .iter()
        .map(|proposer| {
            let timestamp = get_time();
            let registration = ValidatorRegistration {
                fee_recipient: proposer.fee_recipient.clone(),
                gas_limit: 30_000_000,
                timestamp,
                public_key: proposer.validator.public_key.clone(),
            };
            let signature =
                sign_builder_message(&registration, &proposer.signing_key, &context).unwrap();
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
    let fork = if shuffling_index == 0 { Fork::Bellatrix } else { Fork::Capella };
    let current_slot = match fork {
        Fork::Bellatrix => 30 + context.bellatrix_fork_epoch * context.slots_per_epoch,
        Fork::Capella => 30 + context.capella_fork_epoch * context.slots_per_epoch,
        _ => unimplemented!(),
    };
    let parent_hash = Hash32::try_from([shuffling_index as u8; 32].as_ref()).unwrap();

    let request = AuctionRequest {
        slot: current_slot,
        parent_hash: parent_hash.clone(),
        public_key: proposer.validator.public_key.clone(),
    };
    let signed_bid = beacon_node.fetch_best_bid(&request).await.unwrap();
    let bid_parent_hash = signed_bid.message.header().parent_hash();
    assert_eq!(bid_parent_hash, &parent_hash);

    let signed_block = match fork {
        Fork::Bellatrix => {
            let header = signed_bid.message.header().bellatrix().unwrap().clone();
            let beacon_block_body = bellatrix::BlindedBeaconBlockBody {
                execution_payload_header: header,
                ..Default::default()
            };
            let beacon_block = bellatrix::BlindedBeaconBlock {
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
            let signature = sign_with_domain(&beacon_block, &proposer.signing_key, domain).unwrap();
            let signed_block =
                bellatrix::SignedBlindedBeaconBlock { message: beacon_block, signature };
            SignedBlindedBeaconBlock::Bellatrix(signed_block)
        }
        Fork::Capella => {
            let header = signed_bid.message.header().capella().unwrap().clone();
            let beacon_block_body = capella::BlindedBeaconBlockBody {
                execution_payload_header: header,
                ..Default::default()
            };
            let beacon_block = capella::BlindedBeaconBlock {
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
            let signature = sign_with_domain(&beacon_block, &proposer.signing_key, domain).unwrap();
            let signed_block =
                capella::SignedBlindedBeaconBlock { message: beacon_block, signature };
            SignedBlindedBeaconBlock::Capella(signed_block)
        }
        _ => unimplemented!(),
    };

    beacon_node.check_status().await.unwrap();

    let auction_contents = beacon_node.open_bid(&signed_block).await.unwrap();
    let payload = auction_contents.execution_payload();

    let payload_parent_hash = payload.parent_hash();
    assert_eq!(payload_parent_hash, &parent_hash);

    let payload_fee_recipient = payload.fee_recipient();
    assert_eq!(payload_fee_recipient, &proposer.fee_recipient);

    beacon_node.check_status().await.unwrap();
}
