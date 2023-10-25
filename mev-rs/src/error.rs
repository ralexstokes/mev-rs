use crate::types::BidRequest;
use beacon_api_client::Error as ApiError;
use ethereum_consensus::{
    primitives::{BlsPublicKey, ExecutionAddress, Hash32, Slot},
    Error as ConsensusError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("bid public key {bid} does not match relay public key {relay}")]
    BidPublicKeyMismatch { bid: BlsPublicKey, relay: BlsPublicKey },
    #[error("no bid prepared for request {0}")]
    NoBidPrepared(Box<BidRequest>),
    #[error("no valid bids returned for proposal")]
    NoBids,
    #[error("could not find relay with outstanding bid to accept")]
    MissingOpenBid,
    #[error("could not find proposer for slot {0}")]
    MissingProposer(Slot),
    #[error("could not register with any relay")]
    CouldNotRegister,
    #[error("no preferences found for validator with public key {0:?}")]
    MissingPreferences(BlsPublicKey),
    #[error("no payload returned for opened bid with block hash {0:?}")]
    MissingPayload(Hash32),
    #[error("payload gas limit does not match the proposer's preference")]
    InvalidGasLimit,
    #[error("data for an unexpected fork was provided")]
    InvalidFork,
    #[error("block does not match the provided header")]
    UnknownBlock,
    #[error("payload request does not match any outstanding bid")]
    UnknownBid,
    #[error("validator {0:?} does not have {1:?} fee recipient")]
    UnknownFeeRecipient(BlsPublicKey, ExecutionAddress),
    #[error("validator with public key {0:?} is not currently registered")]
    ValidatorNotRegistered(BlsPublicKey),
    #[error(transparent)]
    ValidatorRegistry(#[from] crate::validator_registry::Error),
    #[error(transparent)]
    ProposerScheduler(#[from] crate::proposer_scheduler::Error),
    #[error(transparent)]
    Consensus(#[from] ConsensusError),
    #[error(transparent)]
    Api(#[from] ApiError),
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
