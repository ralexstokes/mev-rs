mod cmd;

use clap::{Parser, Subcommand};
use std::future::Future;
use tokio::signal;
use tracing::warn;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(feature = "build")]
use ::{clap::CommandFactory, eyre::OptionExt, std::path::PathBuf};

const MINIMAL_PRESET_NOTICE: &str =
    "`minimal-preset` feature is enabled. The `minimal` consensus preset is being used.";

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

    if cfg!(feature = "minimal-preset") {
        warn!("{MINIMAL_PRESET_NOTICE}");
    }

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

#[cfg(feature = "build")]
fn parse_custom_chain_config_directory() -> eyre::Result<Option<PathBuf>> {
    let matches = Cli::command().get_matches();
    let (_, matches) = matches.subcommand().ok_or_eyre("missing subcommand")?;
    let iter = matches.try_get_raw("chain").transpose();

    if let Some(Ok(mut iter)) = iter {
        Ok(iter.next().and_then(|raw| {
            raw.to_str().and_then(|s| {
                let path = PathBuf::from(s);
                path.parent().map(PathBuf::from)
            })
        }))
    } else {
        Ok(None)
    }
}

fn main() -> eyre::Result<()> {
    #[cfg(feature = "build")]
    let custom_chain_config_directory = parse_custom_chain_config_directory()?;

    let cli = Cli::parse();

    match cli.command {
        #[cfg(feature = "boost")]
        Commands::Boost(cmd) => run_task_until_signal(cmd.execute()),
        #[cfg(feature = "build")]
        Commands::Build(cmd) => cmd.run(|node_builder, cli_args| async move {
            if cfg!(feature = "minimal-preset") {
                warn!("{MINIMAL_PRESET_NOTICE}");
            }
            let config: cmd::config::Config = cli_args.try_into()?;
            if let Some(network) = config.network {
                warn!(%network, "`network` option provided in configuration but ignored in favor of `reth` configuration");
            }
            let config = config.builder.ok_or_eyre("missing `builder` configuration")?;
            mev_build_rs::launch(node_builder, custom_chain_config_directory,  config).await
        }),
        #[cfg(feature = "relay")]
        Commands::Relay(cmd) => run_task_until_signal(cmd.execute()),
        Commands::Config(cmd) => run_task_until_signal(cmd.execute()),
    }
}
