use anyhow::{Context, Result};
use clap::Args;
use mev_boost_rs::Config as BoostConfig;
use mev_build_rs::Config as BuildConfig;
use mev_relay_rs::Config as RelayConfig;
use mev_rs::Network;
use serde::Deserialize;
use std::{fmt, path::Path};

#[derive(Debug, Deserialize)]
pub struct Config {
    pub boost: Option<BoostConfig>,
    pub builder: Option<BuildConfig>,
    pub relay: Option<RelayConfig>,
}

impl Config {
    pub fn from_toml_file<P: AsRef<Path> + fmt::Display + Clone>(path: P) -> Result<Config> {
        tracing::info!("loading config from `{path}`...");

        let config_data = std::fs::read(path.as_ref())
            .with_context(|| format!("could not read config from `{path}`"))?;

        toml::from_slice(&config_data).context("could not parse TOML")
    }
}

#[derive(Debug, Args)]
#[clap(about = "🔬 (debug) utility to verify configuration")]
pub struct Command {
    #[clap(env)]
    config_file: String,
}

impl Command {
    pub async fn execute(&self, network: Network) -> Result<()> {
        let config_file = &self.config_file;

        let config = Config::from_toml_file(config_file)?;

        tracing::info!("configured for network `{}` with configuration {:#?}", network, config);

        Ok(())
    }
}
