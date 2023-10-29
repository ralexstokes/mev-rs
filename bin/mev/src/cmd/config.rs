use clap::Args;
use ethereum_consensus::networks::Network;
use eyre::WrapErr;
#[cfg(feature = "boost")]
use mev_boost_rs::Config as BoostConfig;
#[cfg(feature = "build")]
use mev_build_rs::reth_builder::Config as BuildConfig;
#[cfg(feature = "relay")]
use mev_relay_rs::Config as RelayConfig;
use mev_rs::config::from_toml_file;
use serde::Deserialize;
use std::{fmt, path::Path};
use tracing::info;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub network: Network,
    #[cfg(feature = "boost")]
    pub boost: Option<BoostConfig>,
    #[cfg(feature = "build")]
    #[serde(rename = "builder")]
    pub build: Option<BuildConfig>,
    #[cfg(feature = "relay")]
    pub relay: Option<RelayConfig>,
}

impl Config {
    pub fn from_toml_file<P: AsRef<Path> + fmt::Display>(path: P) -> eyre::Result<Config> {
        tracing::info!("loading config from `{path}`...");

        from_toml_file::<_, Self>(path.as_ref()).wrap_err("could not parse TOML")
    }
}

#[derive(Debug, Args)]
#[clap(about = "🔬 (debug) utility to verify configuration")]
pub struct Command {
    #[clap(env)]
    config_file: String,
}

impl Command {
    pub async fn execute(self) -> eyre::Result<()> {
        let config_file = self.config_file;

        let config = Config::from_toml_file(config_file)?;
        info!("{config:#?}");

        Ok(())
    }
}
