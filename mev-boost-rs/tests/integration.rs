use beacon_api_client::Client as ApiClient;
use ethereum_consensus::bellatrix::mainnet::{
    BlindedBeaconBlock, BlindedBeaconBlockBody, SignedBlindedBeaconBlock,
};
use ethereum_consensus::builder::{
    compute_builder_domain, SignedValidatorRegistration, ValidatorRegistration,
};
use ethereum_consensus::crypto::SecretKey;
use ethereum_consensus::phase0::mainnet::{Context, Validator};
use ethereum_consensus::phase0::sign_with_domain;
use ethereum_consensus::primitives::{BlsSignature, ExecutionAddress, Hash32, Slot};
use mev_boost_rs::{Config, Service};
use mev_build_rs::{ApiServer, BidRequest, Builder};
use mev_relay_rs::{Client as RelayClient, Relay};
use rand;
use rand::seq::SliceRandom;
use ssz_rs::prelude::SimpleSerialize;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

fn setup_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

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

            let mut validator = Validator::default();
            validator.pubkey = public_key;

            let fee_recipient = ExecutionAddress::try_from_bytes(&[i as u8; 20]).unwrap();

            Proposer {
                index: i,
                validator,
                signing_key,
                fee_recipient,
            }
        })
        .collect()
}

fn sign_message<T: SimpleSerialize>(
    message: &mut T,
    signing_key: &SecretKey,
    context: &Context,
) -> BlsSignature {
    let domain = compute_builder_domain(context).unwrap();
    sign_with_domain(message, signing_key, domain).unwrap()
}

#[tokio::test]
async fn test_end_to_end() {
    setup_logging();

    // start upstream relay
    let port = 28545;
    let relay = Relay::default();
    let relay_server = ApiServer::new("127.0.0.1".parse().unwrap(), port, relay);
    tokio::spawn(async move { relay_server.run().await });

    // start mux server
    let mut config = Config::default();
    config.relays.push(format!("http://127.0.0.1:{port}"));

    let mux_port = config.port;
    let service = Service::from(config);
    tokio::spawn(async move { service.run().await });

    // let other tasks run so servers boot before we proceed
    tokio::task::yield_now().await;

    let beacon_node = RelayClient::new(ApiClient::new(
        Url::parse(format!("http://127.0.0.1:{mux_port}")).unwrap(),
    ));

    let mut rng = rand::thread_rng();

    let mut proposers = create_proposers(&mut rng, 4);

    beacon_node.check_status().await.unwrap();

    let context = Context::for_mainnet();
    for proposer in &proposers {
        let timestamp = get_time();
        let mut registration = ValidatorRegistration {
            fee_recipient: proposer.fee_recipient.clone(),
            gas_limit: 30_000_000,
            timestamp,
            public_key: proposer.validator.pubkey.clone(),
        };
        let signature = sign_message(&mut registration, &proposer.signing_key, &context);
        let signed_registration = SignedValidatorRegistration {
            message: registration,
            signature,
        };
        beacon_node
            .register_validator(&signed_registration)
            .await
            .unwrap();
    }

    beacon_node.check_status().await.unwrap();

    proposers.shuffle(&mut rng);

    for (i, proposer) in proposers.iter().enumerate() {
        propose_block(&beacon_node, proposer, i, &context).await;
    }
}

async fn propose_block(
    beacon_node: &RelayClient,
    proposer: &Proposer,
    shuffling_index: usize,
    context: &Context,
) {
    let current_slot = 32 + shuffling_index as Slot;
    let parent_hash = Hash32::try_from_bytes(&[shuffling_index as u8; 32]).unwrap();

    let request = BidRequest {
        slot: current_slot,
        parent_hash: parent_hash.clone(),
        public_key: proposer.validator.pubkey.clone(),
    };
    let signed_bid = beacon_node.fetch_best_bid(&request).await.unwrap();
    let bid = &signed_bid.message;
    assert_eq!(bid.header.parent_hash, parent_hash);

    let beacon_block_body = BlindedBeaconBlockBody {
        execution_payload_header: bid.header.clone(),
        ..Default::default()
    };
    let mut beacon_block = BlindedBeaconBlock {
        slot: current_slot,
        proposer_index: proposer.index,
        body: beacon_block_body,
        ..Default::default()
    };
    let signature = sign_message(&mut beacon_block, &proposer.signing_key, context);
    let signed_block = SignedBlindedBeaconBlock {
        message: beacon_block,
        signature,
    };

    beacon_node.check_status().await.unwrap();

    let payload = beacon_node.open_bid(&signed_block).await.unwrap();

    assert_eq!(payload.parent_hash, parent_hash);
    assert_eq!(payload.fee_recipient, proposer.fee_recipient);

    beacon_node.check_status().await.unwrap();
}
