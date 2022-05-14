use crate::types::{BidRequest, SignedBuilderBid};
use async_trait::async_trait;
use beacon_api_client::ApiError;
use ethereum_consensus::bellatrix::mainnet::{ExecutionPayload, SignedBlindedBeaconBlock};
use ethereum_consensus::builder::SignedValidatorRegistration;
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
