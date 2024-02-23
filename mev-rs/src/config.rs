use std::{io, path::Path};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Toml(#[from] toml::de::Error),
}

pub fn from_toml_file<P: AsRef<Path>, T: serde::de::DeserializeOwned>(path: P) -> Result<T, Error> {
    let config_data = std::fs::read_to_string(path.as_ref())?;

    toml::from_str(&config_data).map_err(From::from)
}
