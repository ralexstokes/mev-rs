mod cmd;

use clap::{Parser, Subcommand};
use std::future::Future;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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

fn run_task_until_signal(task: impl Future<Output = eyre::Result<()>>) -> eyre::Result<()> {
    setup_logging();

    // impl #[tokio::main]
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("can make runtime")
        .block_on(async move {
            tokio::select! {
                task = task => task,
                _ = signal::ctrl_c() => {
                    tracing::info!("shutting down...");
                    Ok(())
                }
            }
        })
}

fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        #[cfg(feature = "boost")]
        Commands::Boost(cmd) => run_task_until_signal(cmd.execute()),
        #[cfg(feature = "build")]
        Commands::Build(cmd) => cmd.run(|node_builder, cli_args| async move {
            let config: cmd::config::Config = cli_args.try_into()?;
            let network = config.network;

            if let Some(config) = config.builder {
                mev_build_rs::launch(node_builder, network, config).await
            } else {
                Err(eyre::eyre!("missing `builder` configuration"))
            }
        }),
        #[cfg(feature = "relay")]
        Commands::Relay(cmd) => run_task_until_signal(cmd.execute()),
        Commands::Config(cmd) => run_task_until_signal(cmd.execute()),
    }
}
