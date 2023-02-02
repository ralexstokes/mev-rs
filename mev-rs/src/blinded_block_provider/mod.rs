#[cfg(feature = "api")]
mod api;

use crate::{
    types::{
        BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
    validator_registration::validator_registrar::Error as ValidatorRegistrationError,
};

use async_trait::async_trait;
use beacon_api_client::ApiError;
use ethereum_consensus::{primitives::BlsPublicKey, state_transition::Error as ConsensusError};
use thiserror::Error;
#[cfg(feature = "api")]
pub use {api::client::Client, api::server::Server, beacon_api_client::Error as ClientError};

#[derive(Debug, Error)]
pub enum Error {
    #[error("no bid prepared for request {0:?}")]
    NoBidPrepared(Box<BidRequest>),
    #[error("missing preferences for validator with public key {0}")]
    MissingPreferences(BlsPublicKey),
    #[error("no header prepared for request: {0:?}")]
    NoHeaderPrepared(Box<BidRequest>),
    #[error("no payload prepared for request: {0:?}")]
    NoPayloadPrepared(Box<BidRequest>),
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    ValidatorRegistration(#[from] ValidatorRegistrationError),
    #[error("internal server error")]
    Internal(String),
    #[error("{0}")]
    Custom(String),
}

#[cfg(feature = "api")]
impl From<ClientError> for Error {
    fn from(err: ClientError) -> Self {
        match err {
            ClientError::Api(err) => err.into(),
            err => Error::Internal(err.to_string()),
        }
    }
}

#[async_trait]
pub trait BlindedBlockProvider {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error>;

    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error>;

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error>;
}
