use clap::Parser;
use mev_boost_rs::{Service, ServiceConfig};
use std::net::Ipv4Addr;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_HOST: Ipv4Addr = Ipv4Addr::LOCALHOST;
const DEFAULT_PORT: u16 = 18550;
const DEFAULT_RELAY_URL: &str = "127.0.0.1:8080";

#[derive(Parser, Debug)]
#[clap(version, about, long_about=None)]
struct Args {
    #[clap(long, default_value_t = DEFAULT_HOST)]
    host: Ipv4Addr,

    #[clap(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    #[clap(long, default_value = DEFAULT_RELAY_URL )]
    /// a comma-separated list of relay endpoints
    relays: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let config = ServiceConfig {
        host: args.host,
        port: args.port,
        relays: args
            .relays
            .split(",")
            .filter_map(|relay| {
                relay
                    .parse()
                    .map_err(|err| {
                        tracing::warn!("{err}");
                        err
                    })
                    .ok()
            })
            .collect(),
    };
    let mut service = Service::from(config);
    service.run().await;
}
