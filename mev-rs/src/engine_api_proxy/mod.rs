pub mod client;
pub mod server;
pub mod types;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unexpected data when decoding reseponse")]
    UnexpectedResponse,
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Http(#[from] reqwest::Error),
}
