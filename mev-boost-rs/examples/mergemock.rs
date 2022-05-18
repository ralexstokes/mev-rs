/// This example runs an end-to-end integration test but requires caller to
/// have [mergemock](https://github.com/protolambda/mergemock) running locally at commit
/// `640bdc11a9e4ffac83a06ed87aa345466ad7a540`.
///
/// after building `mergemock`:
/// `./mergemock relay` in another process and then run this example:
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
use ethereum_consensus::primitives::{BlsSignature, ExecutionAddress, Hash32};
use hex;
use mev_boost_rs::{Config, Service};
use mev_build_rs::{BidRequest, Builder};
use mev_relay_rs::Client as RelayClient;
use rand;
use rand::seq::SliceRandom;
use serde_json::{self, Value};
use ssz_rs::prelude::SimpleSerialize;
use std::time::{SystemTime, UNIX_EPOCH};
use url::Url;

fn setup_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
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

async fn get_parent_params(client: &ApiClient) -> (u64, String) {
    let inputs = r#"{
        "jsonrpc":"2.0",
        "method":"eth_getBlockByNumber",
        "params":["latest", false],
        "id":1
    }"#;
    let input_value: Value = serde_json::from_str(inputs).unwrap();
    let response: Value = client
        .http_post("/", &input_value)
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let number_data = response
        .pointer("/result/number")
        .unwrap()
        .as_str()
        .unwrap();
    let hash = response.pointer("/result/hash").unwrap().as_str().unwrap();
    let number = u64::from_str_radix(&number_data[2..], 16).unwrap();
    (number, hash.to_string())
}

async fn update_mergemock(client: &ApiClient, parent_hash: &str, fee_recipient: &ExecutionAddress) {
    let inputs = format!(
        r#"{{
        "jsonrpc":"2.0",
        "method":"engine_forkchoiceUpdatedV1",
        "params":[
            {{
                "headBlockHash": "{0}",
                "safeBlockHash": "{0}",
                "finalizedBlockHash": "{0}"
            }},
            {{
                "timestamp": "0x1234",
                "prevRandao": "0xc2fa210081542a87f334b7b14a2da3275e4b281dd77b007bcfcb10e34c42052e",
                "suggestedFeeRecipient": "{1}"
            }}
        ],
        "id":1
    }}"#,
        parent_hash, fee_recipient,
    );
    let input_value: Value = serde_json::from_str(&inputs).unwrap();
    let _ = client.http_post("/", &input_value).await.unwrap();
}

async fn get_latest_block_params(
    client: &ApiClient,
    fee_recipient: &ExecutionAddress,
) -> (u64, Hash32) {
    let (number, parent_hash) = get_parent_params(client).await;

    update_mergemock(client, &parent_hash, fee_recipient).await;

    let parent_hash = hex::decode(&parent_hash[2..]).unwrap();
    (number + 1, Hash32::try_from_bytes(&parent_hash).unwrap())
}

#[tokio::main]
async fn main() {
    setup_logging();

    // start mux server
    let mergemock_port = 28545;
    let relay_url = format!("http://127.0.0.1:{mergemock_port}");
    let mut config = Config::default();
    config.relays.push(relay_url);
    let relay_client = ApiClient::new(Url::parse("http://127.0.0.1:8551").unwrap());

    let mux_port = config.port;
    let service = Service::from(config);
    tokio::spawn(async move { service.run().await });

    // let other tasks run so servers boot before we proceed
    tokio::task::yield_now().await;

    let beacon_node = RelayClient::new(ApiClient::new(
        Url::parse(&format!("http://127.0.0.1:{mux_port}")).unwrap(),
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

    for proposer in &proposers {
        propose_block(&beacon_node, proposer, &context, &relay_client).await;
    }
}

async fn propose_block(
    beacon_node: &RelayClient,
    proposer: &Proposer,
    context: &Context,
    relay_client: &ApiClient,
) {
    let (block_number, parent_hash) =
        get_latest_block_params(relay_client, &proposer.fee_recipient).await;
    let current_slot = block_number;

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

    dbg!(payload);

    beacon_node.check_status().await.unwrap();
}
