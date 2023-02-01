use crate::cmd::config::Config;
use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use mev_build_rs::Service;
use mev_rs::Network;

#[derive(Debug, Args)]
#[clap(about = "🛠️ building blocks since 2023", subcommand_negates_reqs = true)]
pub struct Command {
    #[clap(env, required = true)]
    config_file: Option<String>,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Mempool { config_file: String },
}

impl Command {
    pub async fn execute(&self, network: Network) -> Result<()> {
        let config_file = if let Some(subcommand) = self.command.as_ref() {
            match subcommand {
                Commands::Mempool { config_file } => config_file,
            }
        } else {
            self.config_file.as_ref().unwrap()
        };

        let config = Config::from_toml_file(config_file)?;

        if let Some(mut config) = config.builder {
            config.network = network;
            let service = Service::from(config).spawn(None).await?;
            Ok(service.await?)
        } else {
            Err(anyhow!("missing builder config from file provided"))
        }
    }
}
