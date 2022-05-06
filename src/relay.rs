use crate::types::{
    BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use beacon_api_client::{Client as BeaconApiClient, Error};

pub struct Relay {
    api: BeaconApiClient,
}

impl Relay {
    pub fn new(api_client: BeaconApiClient) -> Self {
        Self { api: api_client }
    }

    pub async fn register_validator(
        &self,
        registration: &SignedValidatorRegistration,
    ) -> Result<(), Error> {
        self.api
            .post("/eth/v1/builder/validators", registration)
            .await
    }

    pub async fn fetch_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error> {
        let target = format!(
            "/eth/v1/builder/header/{}/{}/{}",
            bid_request.slot, bid_request.parent_hash, bid_request.public_key
        );
        self.api.get(&target).await
    }

    pub async fn accept_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        self.api
            .post("/eth/v1/builder/blinded_blocks", signed_block)
            .await
    }
}
