use beacon_api_client::Client as ApiClient;
use ethereum_consensus::bellatrix::mainnet::{
    BlindedBeaconBlock, BlindedBeaconBlockBody, SignedBlindedBeaconBlock,
};
use ethereum_consensus::builder::{SignedValidatorRegistration, ValidatorRegistration};
use ethereum_consensus::crypto::SecretKey;
use ethereum_consensus::phase0::mainnet::Validator;
use ethereum_consensus::primitives::{ExecutionAddress, Hash32, Slot};
use mev_boost_rs::{Config, Service};
use mev_build_rs::{ApiServer, BidRequest, Builder};
use mev_relay_rs::{Client as RelayClient, Relay};
use rand;
use rand::seq::SliceRandom;
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

struct Proposer {
    index: usize,
    validator: Validator,
    _signing_key: SecretKey,
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
                _signing_key: signing_key,
                fee_recipient,
            }
        })
        .collect()
}

#[tokio::test]
async fn test_end_to_end() {
    setup_logging();

    // start upstream relay
    let relay = Relay::default();
    let relay_server = ApiServer::new("127.0.0.1".parse().unwrap(), 8080, relay);
    tokio::spawn(async move { relay_server.run().await });

    // start mux server
    let mut config = Config::default();
    config.relays.push("http://127.0.0.1:8080".to_string());

    let service = Service::from(config);
    tokio::spawn(async move { service.run().await });

    // let other tasks run so servers boot before we proceed
    tokio::task::yield_now().await;

    let beacon_node = RelayClient::new(ApiClient::new(
        Url::parse("http://127.0.0.1:18550").unwrap(),
    ));

    let mut rng = rand::thread_rng();

    let mut proposers = create_proposers(&mut rng, 4);

    beacon_node.check_status().await.unwrap();

    for proposer in &proposers {
        let registration = ValidatorRegistration {
            fee_recipient: proposer.fee_recipient.clone(),
            public_key: proposer.validator.pubkey.clone(),
            ..Default::default()
        };
        let signed_registration = SignedValidatorRegistration {
            message: registration,
            ..Default::default()
        };
        beacon_node
            .register_validator(&signed_registration)
            .await
            .unwrap();
    }

    beacon_node.check_status().await.unwrap();

    proposers.shuffle(&mut rng);

    for (i, proposer) in proposers.iter().enumerate() {
        propose_block(&beacon_node, proposer, i).await;
    }
}

async fn propose_block(beacon_node: &RelayClient, proposer: &Proposer, shuffling_index: usize) {
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
    let beacon_block = BlindedBeaconBlock {
        slot: current_slot,
        proposer_index: proposer.index,
        body: beacon_block_body,
        ..Default::default()
    };
    // TODO sign full block
    let signed_block = SignedBlindedBeaconBlock {
        message: beacon_block,
        ..Default::default()
    };

    beacon_node.check_status().await.unwrap();

    let payload = beacon_node.open_bid(&signed_block).await.unwrap();

    assert_eq!(payload.parent_hash, parent_hash);
    assert_eq!(payload.fee_recipient, proposer.fee_recipient);

    beacon_node.check_status().await.unwrap();
}
