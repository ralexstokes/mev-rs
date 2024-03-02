use clap::{Parser, Subcommand};
use std::future::Future;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod cmd;

#[derive(Debug, Parser)]
#[clap(author, version, about = "utilities for block space", long_about = None)]
struct Cli {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    #[cfg(feature = "boost")]
    Boost(cmd::boost::Command),
    #[cfg(feature = "build")]
    Build(cmd::build::Command),
    #[cfg(feature = "relay")]
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

async fn run_task_until_signal(task: impl Future<Output = eyre::Result<()>>) -> eyre::Result<()> {
    setup_logging();

    tokio::select! {
        task = task => task,
        _ = signal::ctrl_c() => {
            tracing::info!("shutting down...");
            Ok(())
        }
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        #[cfg(feature = "boost")]
        Commands::Boost(cmd) => run_task_until_signal(cmd.execute()).await,
        #[cfg(feature = "build")]
        Commands::Build(cmd) => tokio::task::block_in_place(|| cmd.run()),
        #[cfg(feature = "relay")]
        Commands::Relay(cmd) => run_task_until_signal(cmd.execute()).await,
        Commands::Config(cmd) => run_task_until_signal(cmd.execute()).await,
    }
}
