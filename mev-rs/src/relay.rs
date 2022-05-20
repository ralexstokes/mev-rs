use crate::config::Config;
use anyhow::{anyhow, Result};
use clap::{Args, Subcommand};
use mev_relay_rs::Service;

#[derive(Debug, Args)]
#[clap(
    about = "🏗 connecting builders to proposers",
    subcommand_negates_reqs = true
)]
pub(crate) struct Command {
    #[clap(env, required = true)]
    config_file: Option<String>,

    #[clap(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    Mock { config_file: String },
}

impl Command {
    pub(crate) async fn execute(&self) -> Result<()> {
        let (config_file, _mock) = if let Some(subcommand) = self.command.as_ref() {
            match subcommand {
                Commands::Mock { config_file } => (config_file, true),
            }
        } else {
            (self.config_file.as_ref().unwrap(), false)
        };

        let config = Config::from_toml_file(config_file)?;

        if let Some(config) = config.relay {
            // TODO separate mock and "real" modes
            Service::from(config).run().await;
            Ok(())
        } else {
            Err(anyhow!("missing relay config from file provided"))
        }
    }
}
