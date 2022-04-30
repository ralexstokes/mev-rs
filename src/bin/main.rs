use mev_boost_rs::{Service, ServiceConfig};
use std::net::Ipv4Addr;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_HOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
const DEFAULT_PORT: u16 = 18550;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = ServiceConfig {
        host: DEFAULT_HOST,
        port: DEFAULT_PORT,
        relays: vec!["127.0.0.1:8080".parse().unwrap()],
    };
    let mut service = Service::from(config);
    service.run().await;
}
