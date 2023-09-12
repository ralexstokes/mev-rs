mod cmd;

use anyhow::Result;
use clap::{ArgGroup, Parser, Subcommand};
use mev_rs::Network;
use std::future::Future;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Default, Debug, Clone, clap::ValueEnum)]
pub enum NetworkArg {
    #[default]
    Mainnet,
    Sepolia,
    Goerli,
}

// NOTE: define this mapping so only this crate needs the `clap` dependency while still being able
// to use the `clap::ValueEnum` machinery
impl From<NetworkArg> for Network {
    fn from(arg: NetworkArg) -> Self {
        match arg {
            NetworkArg::Mainnet => Self::Mainnet,
            NetworkArg::Sepolia => Self::Sepolia,
            NetworkArg::Goerli => Self::Goerli,
        }
    }
}

#[derive(Debug, Parser)]
#[clap(author, version, about = "utilities for block space", long_about = None)]
#[clap(group(ArgGroup::new("network-config").args(&["network", "network_config"])))]
struct Cli {
    #[clap(long, value_enum, value_name = "NETWORK")]
    network: Option<NetworkArg>,
    #[clap(long, value_name = "FILE")]
    network_config: Option<String>,
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Boost(cmd::boost::Command),
    Build(cmd::build::Command),
    Relay(cmd::relay::Command),
    Config(cmd::config::Command),
}

fn setup_logging() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
}

async fn run_task_until_signal(task: impl Future<Output = Result<()>>) -> Result<()> {
    tokio::select! {
        task = task => task,
        _ = signal::ctrl_c() => {
            tracing::info!("shutting down...");
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let network = if let Some(network) = cli.network {
        network.into()
    } else if let Some(network_config) = cli.network_config {
        // use custom config if provided
        Network::Custom(network_config)
    } else {
        // default to `mainnet` if no network configuration is provided
        let network = NetworkArg::Mainnet;
        network.into()
    };

    setup_logging();

    tracing::info!("configured for {network}");

    match cli.command {
        Commands::Boost(cmd) => run_task_until_signal(cmd.execute(network)).await,
        Commands::Build(cmd) => run_task_until_signal(cmd.execute(network)).await,
        Commands::Relay(cmd) => run_task_until_signal(cmd.execute(network)).await,
        Commands::Config(cmd) => run_task_until_signal(cmd.execute(network)).await,
    }
}
