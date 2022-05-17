use clap::Parser;
use mev_boost_rs::{Config, Service};
use std::fs;
use tokio::signal;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Debug, Parser)]
#[clap(version, about, long_about=None)]
struct Args {
    #[clap(long, env, default_value = "config.toml")]
    config_file: String,
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

    let config_file = &args.config_file;
    let config_data = match fs::read(config_file) {
        Ok(file) => file,
        Err(err) => {
            tracing::error!("could not read file `{config_file}`: {err}");
            return;
        }
    };
    let config: Config = match toml::from_slice(&config_data) {
        Ok(config) => config,
        Err(err) => {
            tracing::error!("could not parse TOML: {err}");
            return;
        }
    };

    let service = Service::from(config);
    tokio::select! {
        _ = service.run()=> {},
        _ = signal::ctrl_c() => {},
    }
}
