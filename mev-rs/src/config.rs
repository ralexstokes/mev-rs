use anyhow::{Context, Result};
use mev_boost_rs::Config as BoostConfig;
use mev_relay_rs::Config as RelayConfig;
use serde::Deserialize;
use std::fmt;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub(crate) struct Config {
    pub(crate) boost: Option<BoostConfig>,
    pub(crate) relay: Option<RelayConfig>,
}

impl Config {
    pub fn from_toml_file<P: AsRef<Path> + fmt::Display + Clone>(path: P) -> Result<Config> {
        let config_data = std::fs::read(path.as_ref())
            .with_context(|| format!("could not read config from `{path}`"))?;

        toml::from_slice(&config_data).context("could not parse TOML")
    }
}
