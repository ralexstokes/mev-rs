#[cfg(feature = "api")]
mod api;

use crate::{
    error::Error,
    types::{
        BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
};
use async_trait::async_trait;
#[cfg(feature = "api")]
pub use {api::client::Client, api::server::Server};

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
