#[cfg(feature = "builder-api")]
mod api;

#[cfg(feature = "builder-api")]
pub use {api::client::Client, api::server::Server};

use crate::{
    error::Error,
    types::{
        BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
};
use async_trait::async_trait;

#[async_trait]
pub trait ValidatorTrait {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error>;
}
#[async_trait]
pub trait BidderTrait {
    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error>;

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error>;
}
