use beacon_api_client::Client as ApiClient;
use ethereum_consensus::bellatrix::SignedBlindedBeaconBlock;
use ethereum_consensus::builder::SignedValidatorRegistration;
use mev_boost_rs::{
    relay_server::Server, BidRequest, Relay as RelayClient, Service, ServiceConfig,
};
use tokio::time::{self, Duration};
use url::Url;

use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::test]
async fn test_end_to_end() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut config = ServiceConfig::default();

    let mut relay = Server::new("127.0.0.1".parse().unwrap(), 8080);
    tokio::spawn(async move { relay.run().await });

    config
        .relays
        .push(Url::parse("http://127.0.0.1:8080").unwrap());

    let mut service = Service::from(config);

    tokio::spawn(async move { service.run().await });

    // TODO wait for server boot
    time::sleep(Duration::from_secs(1)).await;

    // TODO:
    // - register some validators
    // - make a simple shuffling
    // - call for headers in shuffle
    // - accept and get full payloads back

    let user = RelayClient::new(ApiClient::new(
        Url::parse("http://127.0.0.1:18550").unwrap(),
    ));

    user.check_status().await.unwrap();

    let registration = SignedValidatorRegistration::default();
    user.register_validator(&registration).await.unwrap();

    user.check_status().await.unwrap();

    let request = BidRequest::default();
    let signed_bid = user.fetch_bid(&request).await.unwrap();

    // TODO make beacon block
    // TODO sign full block

    user.check_status().await.unwrap();

    let signed_block = SignedBlindedBeaconBlock::default();
    let payload = user.accept_bid(&signed_block).await.unwrap();

    user.check_status().await.unwrap();
}
