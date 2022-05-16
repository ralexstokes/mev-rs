use clap::Parser;
use mev_boost_rs::{Service, ServiceConfig};
use std::net::Ipv4Addr;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

const DEFAULT_HOST: Ipv4Addr = Ipv4Addr::UNSPECIFIED;
const DEFAULT_PORT: u16 = 18550;

#[derive(Parser, Debug)]
#[clap(version, about, long_about=None)]
struct Args {
    #[clap(long, default_value_t = DEFAULT_HOST)]
    host: Ipv4Addr,

    #[clap(long, default_value_t = DEFAULT_PORT)]
    port: u16,

    #[clap(long, default_value = "")]
    /// a comma-separated list of relay endpoints
    relays: String,
}

fn parse_relay(input: &str) -> Option<url::Url> {
    if input.is_empty() {
        None
    } else {
        input
            .parse()
            .map_err(|err| {
                tracing::warn!("error parsing relay from config: {err}");
                err
            })
            .ok()
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args = Args::parse();

    let config = ServiceConfig {
        host: args.host,
        port: args.port,
        relays: args.relays.split(',').filter_map(parse_relay).collect(),
    };

    if config.relays.is_empty() {
        tracing::error!("no relays provided, please restart with at least one relay provided")
    }

    let service = Service::from(config);
    service.run().await;
}
