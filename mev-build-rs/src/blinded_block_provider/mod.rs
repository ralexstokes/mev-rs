#[cfg(feature = "api")]
mod api;

#[cfg(feature = "api")]
pub use {api::client::Client, api::server::Server, beacon_api_client::Error as ClientError};

use crate::{
    builder::Error as BuilderError,
    validator_registration::validator_registrar::Error as ValidatorRegistrationError,
    types::{
        BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
};
use async_trait::async_trait;
use beacon_api_client::ApiError;
use ethereum_consensus::state_transition::Error as ConsensusError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    Builder(#[from] BuilderError),
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
