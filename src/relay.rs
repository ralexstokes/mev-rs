use reqwest::{Client, Error, StatusCode};
use std::net::SocketAddr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("{0}")]
    HTTPError(#[from] Error),
    #[error("simple request to relay failed")]
    CouldNotPing,
}

pub struct Relay {
    client: Client,
    endpoint: String,
}

impl Relay {
    pub fn new(client: Client, address: &SocketAddr) -> Self {
        let endpoint = format!("https://{address}");
        Self { client, endpoint }
    }

    pub async fn connect(&mut self) -> Result<(), RelayError> {
        let host = &self.endpoint;
        let endpoint = format!("{host}/");
        let response = self.client.get(endpoint).send().await?;
        if response.status() == StatusCode::OK {
            Ok(())
        } else {
            Err(RelayError::CouldNotPing)
        }
    }
}
