#[cfg(feature = "builder-api")]
mod api;

#[cfg(feature = "builder-api")]
pub use {api::client::Client, api::server::Server};

use crate::{
    error::Error,
    types::{
        AuctionRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
};
use async_trait::async_trait;

#[async_trait]
pub trait BlindedBlockProvider {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error>;

    async fn fetch_best_bid(
        &self,
        auction_request: &AuctionRequest,
    ) -> Result<SignedBuilderBid, Error>;

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error>;
}
