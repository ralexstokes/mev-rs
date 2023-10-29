use crate::cmd::config::Config;
use clap::{Args, Subcommand};
use mev_relay_rs::Service;
use tracing::info;

#[derive(Debug, Args)]
#[clap(about = "🏗 connecting builders to proposers", subcommand_negates_reqs = true)]
pub struct Command {
    #[clap(env, required = true)]
    config_file: Option<String>,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Mock { config_file: String },
}

impl Command {
    pub async fn execute(self) -> eyre::Result<()> {
        let (config_file, _mock) = if let Some(subcommand) = self.command.as_ref() {
            match subcommand {
                Commands::Mock { config_file } => (config_file, true),
            }
        } else {
            (self.config_file.as_ref().unwrap(), false)
        };

        let config = Config::from_toml_file(config_file)?;

        let network = config.network;
        info!("configured for `{network}`");

        if let Some(config) = config.relay {
            let service = Service::from(network, config).spawn().await?;
            Ok(service.await?)
        } else {
            Err(eyre::eyre!("missing relay config from file provided"))
        }
    }
}
