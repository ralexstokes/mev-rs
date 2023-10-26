use crate::types::AuctionRequest;
use beacon_api_client::Error as ApiError;
use ethereum_consensus::{
    primitives::{BlsPublicKey, ExecutionAddress, Hash32, Slot, ValidatorIndex},
    Error as ConsensusError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("bid public key {bid} does not match relay public key {relay}")]
    BidPublicKeyMismatch { bid: BlsPublicKey, relay: BlsPublicKey },
    #[error("no valid bids returned for proposal")]
    NoBids,
    #[error("could not find relay with outstanding bid to accept")]
    MissingOpenBid,
    #[error("could not find proposer for slot {0}")]
    MissingProposer(Slot),
    #[error("could not register with any relay")]
    CouldNotRegister,
    // #[error("no preferences found for validator with public key {0:?}")]
    // MissingPreferences(BlsPublicKey),
    #[error("no payload returned for opened bid with block hash {0:?}")]
    MissingPayload(Hash32),
    #[error("data for an unexpected fork was provided")]
    InvalidFork,

    #[error("execution payload does not match the provided header")]
    InvalidExecutionPayloadInBlock,
    #[error("validator {0:?} does not have registered fee recipient {1:?}")]
    InvalidFeeRecipient(BlsPublicKey, ExecutionAddress),
    #[error("validator {0:?} does not have (adjusted) registered gas limit {1}")]
    InvalidGasLimitForProposer(BlsPublicKey, u64),
    #[error("bid trace declares gas limit of {0:?} but execution payload has {1:?}")]
    InvalidGasLimit(u64, u64),
    #[error("bid trace declares gas usage of {0} but execution payload uses {1}")]
    InvalidGasUsed(u64, u64),
    #[error("bid trace declares parent hash of {0:?} but execution payload has {1:?}")]
    InvalidParentHash(Hash32, Hash32),
    #[error("bid trace declares block hash of {0:?} but execution payload has {1:?}")]
    InvalidBlockHash(Hash32, Hash32),
    #[error("no bid prepared for request {0}")]
    NoBidPrepared(AuctionRequest),

    #[error("missing auction for {0}")]
    MissingAuction(AuctionRequest),
    #[error("signed blinded beacon block is invalid or equivocated")]
    InvalidSignedBlindedBeaconBlock,
    #[error("validator with public key {0:?} is not currently registered")]
    ValidatorNotRegistered(BlsPublicKey),
    #[error("validator with index {0} is not currently registered")]
    ValidatorIndexNotRegistered(ValidatorIndex),
    #[error("builder with public key {0:?} is not currently registered")]
    BuilderNotRegistered(BlsPublicKey),

    #[error(transparent)]
    ValidatorRegistry(#[from] crate::validator_registry::Error),
    #[error(transparent)]
    ProposerScheduler(#[from] crate::proposer_scheduler::Error),
    #[error("validator registration errors: {0:?}")]
    RegistrationErrors(Vec<crate::validator_registry::Error>),

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
