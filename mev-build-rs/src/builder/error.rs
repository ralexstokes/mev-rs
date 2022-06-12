use crate::types::BidRequest as PayloadRequest;
use ethereum_consensus::primitives::{BlsPublicKey, ExecutionAddress};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unknown fee recipient {0}")]
    UnknownFeeRecipient(ExecutionAddress),
    #[error("missing preferences for validator with public key {0}")]
    MissingPreferences(BlsPublicKey),
    #[error("no payload prepared for request: {0:?}")]
    NoPayloadPrepared(PayloadRequest),
    #[error("error with rpc: {0}")]
    Rpc(String),
    #[error("error with http request: {0}")]
    Http(#[from] reqwest::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}
