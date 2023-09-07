use crate::types::BidRequest;
use beacon_api_client::Error as ApiError;
use ethereum_consensus::{
    primitives::{BlsPublicKey, ExecutionAddress, Hash32},
    state_transition::Error as ConsensusError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("bid public key {bid} does not match relay public key {relay}")]
    BidPublicKeyMismatch { bid: BlsPublicKey, relay: BlsPublicKey },
    #[error("no bid prepared for request {0:?}")]
    NoBidPrepared(Box<BidRequest>),
    #[error("no valid bids returned for proposal")]
    NoBids,
    #[error("could not find relay with outstanding bid to accept")]
    MissingOpenBid,
    #[error("could not register with any relay")]
    CouldNotRegister,
    #[error("no preferences found for validator with public key {0}")]
    MissingPreferences(BlsPublicKey),
    #[error("no payload returned for opened bid with block hash {0}")]
    MissingPayload(Hash32),
    #[error("payload gas limit does not match the proposer's preference")]
    InvalidGasLimit,
    #[error("data for an unexpected fork was provided")]
    InvalidFork,
    #[error("block does not match the provided header")]
    UnknownBlock,
    #[error("payload request does not match any outstanding bid")]
    UnknownBid,
    #[error("validator {0} does not have {1} fee recipient")]
    UnknownFeeRecipient(BlsPublicKey, ExecutionAddress),
    #[error("validator with public key {0} is not currently registered")]
    ValidatorNotRegistered(BlsPublicKey),
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    ValidatorRegistry(#[from] crate::validator_registry::Error),
    #[error("{0}")]
    ProposerScheduler(#[from] crate::proposer_scheduler::Error),
    #[cfg(feature = "engine-proxy")]
    #[error("{0}")]
    EngineApi(#[from] crate::engine_api_proxy::Error),
    #[error("invalid slot")]
    InvalidSlot,
    #[error("invalid parent hash")]
    InvalidParentHash,
}

#[cfg(feature = "api")]
use axum::extract::Json;
#[cfg(feature = "api")]
use axum::http::StatusCode;
#[cfg(feature = "api")]
use axum::response::{IntoResponse, Response};

#[cfg(feature = "api")]
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let message = self.to_string();
        let code = StatusCode::BAD_REQUEST;
        (code, Json(beacon_api_client::ApiError::ErrorMessage { code, message })).into_response()
    }
}
