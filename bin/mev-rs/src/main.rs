mod cmd;

use anyhow::Result;
use clap::{Parser, Subcommand};
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
impl From<NetworkArg> for mev_lib::Network {
    fn from(arg: NetworkArg) -> Self {
        match arg {
            NetworkArg::Mainnet => Self::Mainnet,
            NetworkArg::Sepolia => Self::Sepolia,
            NetworkArg::Goerli => Self::Goerli,
        }
    }
}

#[derive(Debug, Parser)]
#[clap(author, version, name = "mev", about = "utilities for block space", long_about = None)]
struct Cli {
    #[clap(long, default_value_t, value_enum)]
    network: NetworkArg,
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Boost(cmd::boost::Command),
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
        _ = signal::ctrl_c() => Ok(()),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    setup_logging();

    let network = cli.network.into();
    match cli.command {
        Commands::Boost(cmd) => run_task_until_signal(cmd.execute(network)).await,
        Commands::Relay(cmd) => run_task_until_signal(cmd.execute(network)).await,
        Commands::Config(cmd) => run_task_until_signal(cmd.execute(network)).await,
    }
}
