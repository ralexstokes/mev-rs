use crate::cmd::config::Config;
use anyhow::{anyhow, Result};
use clap::Args;
use mev_build_rs::reth_builder::Service;
use tracing::info;

#[derive(Debug, Args)]
#[clap(about = "🛠️ building blocks since 2023")]
pub struct Command {
    #[clap(env, default_value = "config.toml")]
    config_file: String,
}

impl Command {
    pub async fn execute(&self) -> Result<()> {
        let config_file = &self.config_file;

        let config = Config::from_toml_file(config_file)?;

        let network = config.network;
        info!("configured for {network}");

        if let Some(config) = config.build {
            Service::from(network, config).spawn().await;
            Ok(())
        } else {
            Err(anyhow!("missing build config from file provided"))
        }
    }
}
