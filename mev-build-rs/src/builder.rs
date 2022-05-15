use crate::types::{
    BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use async_trait::async_trait;
use beacon_api_client::ApiError;
#[cfg(feature = "api")]
use beacon_api_client::Error as ApiClientError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("internal server error")]
    Internal(String),
    #[error("{0}")]
    Custom(String),
}

#[cfg(feature = "api")]
impl From<ApiClientError> for Error {
    fn from(err: ApiClientError) -> Self {
        match err {
            ApiClientError::Api(err) => err.into(),
            err => Error::Internal(err.to_string()),
        }
    }
}

#[async_trait]
pub trait Builder {
    async fn register_validator(
        &self,
        registration: &SignedValidatorRegistration,
    ) -> Result<(), Error>;

    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error>;

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error>;
}
