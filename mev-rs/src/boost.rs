use anyhow::{Context, Result};
use clap::Args;
use mev_boost_rs::{Config, Service};
use std::fs;

#[derive(Debug, Args)]
#[clap(about = "connecting proposers to the external builder network")]
pub(crate) struct Command {
    #[clap(env, default_value = "config.toml")]
    config_file: String,
}

impl Command {
    pub(crate) async fn execute(&self) -> Result<()> {
        let config_file = &self.config_file;

        tracing::info!("loading config from {config_file}...");
        let config_data = fs::read(config_file)
            .with_context(|| format!("could not read config from `{config_file}`"))?;

        let config: Config = toml::from_slice(&config_data).context("could not parse TOML")?;

        Service::from(config).run().await;
        Ok(())
    }
}
