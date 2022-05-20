mod boost;
mod config;
mod relay;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::future::Future;
use tokio::signal;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[derive(Debug, Parser)]
#[clap(author, version, name = "mev", about = "utilities for block space", long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Boost(boost::Command),
    Relay(relay::Command),
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

    match cli.command {
        Commands::Boost(cmd) => run_task_until_signal(cmd.execute()).await?,
        Commands::Relay(cmd) => run_task_until_signal(cmd.execute()).await?,
    }
    Ok(())
}
