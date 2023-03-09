use beacon_api_client::{Client as ApiClient, ValidatorStatus, ValidatorSummary, Value};
use ethereum_consensus::{
    bellatrix::mainnet as bellatrix,
    builder::{SignedValidatorRegistration, ValidatorRegistration},
    capella::mainnet as capella,
    crypto::SecretKey,
    phase0::mainnet::{compute_domain, Validator},
    primitives::{BlsPublicKey, DomainType, ExecutionAddress, Hash32},
    signing::sign_with_domain,
    state_transition::{Context, Forks},
};
use httpmock::prelude::*;
use mev_boost_rs::{Config, Service};
use mev_relay_rs::{Config as RelayConfig, Service as Relay};
use mev_rs::{
    blinded_block_provider::Client as RelayClient,
    signing::sign_builder_message,
    types::{BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid},
};

use rand::seq::SliceRandom;
use std::{
    collections::HashMap,
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

#[tokio::test]
async fn test_end_to_end() {
    setup_logging();

    let mut rng = rand::thread_rng();

    let mut proposers = create_proposers(&mut rng, 4);

    // start mock consensus node
    let validator_mock_server = MockServer::start();
    let balance = 32_000_000_000;
    let validators = proposers
        .iter()
        .map(|proposer| ValidatorSummary {
            index: proposer.index,
            balance,
            status: ValidatorStatus::ActiveOngoing,
            validator: Validator {
                public_key: proposer.signing_key.public_key(),
                effective_balance: balance,
                ..Default::default()
            },
        })
        .collect::<Vec<_>>();
    validator_mock_server.mock(|when, then| {
        when.method(GET).path("/eth/v1/beacon/states/head/validators");
        let response =
            serde_json::to_string(&Value { data: validators, meta: HashMap::new() }).unwrap();
        then.status(200).body(response);
    });

    let mut context = Context::for_mainnet();
    // mock epoch values to transition across forks
    context.bellatrix_fork_epoch = 12;
    context.capella_fork_epoch = 22;

    // start upstream relay
    let validator_mock_server_url = validator_mock_server.url("");
    let relay_config =
        RelayConfig { beacon_node_url: validator_mock_server_url, ..Default::default() };
    let port = relay_config.port;
    let relay = Relay::from(relay_config);
    relay.spawn(Some(context.clone())).await.unwrap();

    // start mux server
    let mut config = Config::default();
    config.relays.push(format!("http://127.0.0.1:{port}"));

    let mux_port = config.port;
    let service = Service::from(config);
    service.spawn(Some(context.clone())).unwrap();

    let beacon_node = RelayClient::new(
        ApiClient::new(Url::parse(&format!("http://127.0.0.1:{mux_port}")).unwrap()),
        BlsPublicKey::default(),
    );
    
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
        propose_block(&beacon_node, proposer, i, &context).await;
    }
}

async fn propose_block(
    beacon_node: &RelayClient,
    proposer: &Proposer,
    shuffling_index: usize,
    context: &Context,
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
            let domain =
                compute_domain(DomainType::BeaconProposer, Some(fork_version), None, context)
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
            let domain =
                compute_domain(DomainType::BeaconProposer, Some(fork_version), None, context)
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
    }

    beacon_node.check_status().await.unwrap();
}
