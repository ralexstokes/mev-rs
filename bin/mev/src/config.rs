use ethereum_consensus::networks::Network;
use eyre::WrapErr;
use mev_rs::config::from_toml_file;
use serde::{Deserialize, Serialize};
use std::{fmt, path::Path};

#[cfg(feature = "boost")]
use mev_boost_rs::Config as BoostConfig;
#[cfg(feature = "build")]
use mev_build_rs::reth_builder::Config as BuildConfig;
#[cfg(feature = "relay")]
use mev_relay_rs::Config as RelayConfig;

#[derive(Debug, Serialize, Deserialize)]
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
