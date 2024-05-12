use crate::types::AuctionRequest;
use beacon_api_client::Error as ApiError;
use ethereum_consensus::{
    crypto::KzgCommitment,
    primitives::{BlsPublicKey, ExecutionAddress, Hash32, ValidatorIndex},
    Error as ConsensusError, Fork,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BoostError {
    #[error("bid public key {bid} does not match relay public key {relay}")]
    BidPublicKeyMismatch { bid: BlsPublicKey, relay: BlsPublicKey },
    #[error("could not find relay with outstanding bid to accept for block {0}")]
    MissingOpenBid(Hash32),
    #[error("could not register with any relay")]
    CouldNotRegister,
    #[error("no payload returned for opened bid with block hash {0:?}")]
    MissingPayload(Hash32),
    #[error("returned payload block hash {provided} did not match expected {expected}")]
    InvalidPayloadHash { expected: Hash32, provided: Hash32 },
    #[error("blobs provided when they were unexpected")]
    InvalidPayloadUnexpectedBlobs,
    #[error(
        "signed block did not match the expected blob commitments ({expected:?} vs {provided:?})"
    )]
    InvalidPayloadBlobs { expected: Vec<KzgCommitment>, provided: Vec<KzgCommitment> },
}

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("received auction request for {0} but no open auction was found")]
    InvalidAuctionRequest(AuctionRequest),
    #[error("execution payload does not match the provided header")]
    InvalidExecutionPayloadInBlock,
    #[error("validator {0:?} does not have registered fee recipient {1:?}")]
    InvalidFeeRecipient(BlsPublicKey, ExecutionAddress),
    // #[error("validator {0:?} does not have (adjusted) registered gas limit {1}")]
    // InvalidGasLimitForProposer(BlsPublicKey, u64),
    #[error("bid trace declares gas limit of {0:?} but execution payload has {1:?}")]
    InvalidGasLimit(u64, u64),
    #[error("bid trace declares gas usage of {0} but execution payload uses {1}")]
    InvalidGasUsed(u64, u64),
    #[error("bid trace declares parent hash of {0:?} but execution payload has {1:?}")]
    InvalidParentHash(Hash32, Hash32),
    #[error("bid trace declares block hash of {0:?} but execution payload has {1:?}")]
    InvalidBlockHash(Hash32, Hash32),
    #[error("missing auction for {0}")]
    MissingAuction(AuctionRequest),
    #[error("signed blinded beacon block is invalid or equivocated")]
    InvalidSignedBlindedBeaconBlock,
    #[error("validator with public key {0:?} is not currently registered")]
    ValidatorNotRegistered(BlsPublicKey),
    #[error("validator with index {0} was not found in consensus")]
    UnknownValidatorIndex(ValidatorIndex),
    #[error("builder with public key {0:?} is not currently registered")]
    BuilderNotRegistered(BlsPublicKey),
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("expecting data from {expected} but provided {provided}")]
    InvalidFork { expected: Fork, provided: Fork },
    #[error("no bid prepared for request {0}")]
    NoBidPrepared(AuctionRequest),
    #[error(transparent)]
    ValidatorRegistry(#[from] crate::validator_registry::Error),
    #[error(transparent)]
    ProposerScheduler(#[from] crate::proposer_scheduler::Error),
    #[error("validator registration errors: {0:?}")]
    RegistrationErrors(Vec<crate::validator_registry::Error>),
    #[error(transparent)]
    Boost(#[from] BoostError),
    #[error(transparent)]
    Relay(#[from] RelayError),
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
        let code = match self {
            Self::NoBidPrepared(..) => StatusCode::NO_CONTENT,
            _ => StatusCode::BAD_REQUEST,
        };
        (code, Json(beacon_api_client::ApiError::ErrorMessage { code, message })).into_response()
    }
}
