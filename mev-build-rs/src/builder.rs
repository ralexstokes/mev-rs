use crate::error::Error;
use crate::types::{
    BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use async_trait::async_trait;

#[async_trait]
pub trait Builder {
    async fn register_validator(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error>;

    async fn fetch_best_bid(&self, bid_request: &mut BidRequest)
        -> Result<SignedBuilderBid, Error>;

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error>;
}
