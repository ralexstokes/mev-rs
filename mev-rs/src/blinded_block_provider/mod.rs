#[cfg(feature = "builder-api")]
pub(crate) mod api;

#[cfg(feature = "builder-api")]
pub use {api::client::Client, api::server::Server};

use crate::{
    error::Error,
    types::{
        AuctionContents, AuctionRequest, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
};
use async_trait::async_trait;

#[async_trait]
pub trait BlindedBlockProvider {
    async fn register_validators(
        &self,
        registrations: &[SignedValidatorRegistration],
    ) -> Result<(), Error>;

    async fn fetch_best_bid(
        &self,
        auction_request: &AuctionRequest,
    ) -> Result<SignedBuilderBid, Error>;

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<AuctionContents, Error>;
}
