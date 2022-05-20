use crate::builder::Builder;
use crate::error::Error;
use crate::types::{
    BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use async_trait::async_trait;
use beacon_api_client::{api_error_or_ok, Client as BeaconApiClient, VersionedValue};

pub struct Client {
    api: BeaconApiClient,
}

impl Client {
    pub fn new(api_client: BeaconApiClient) -> Self {
        Self { api: api_client }
    }

    pub async fn check_status(&self) -> Result<(), beacon_api_client::Error> {
        let response = self.api.http_get("/eth/v1/builder/status").await?;
        api_error_or_ok(response).await
    }
}

#[async_trait]
impl Builder for Client {
    async fn register_validator(
        &self,
        registration: &mut SignedValidatorRegistration,
    ) -> Result<(), Error> {
        let response = self
            .api
            .http_post("/eth/v1/builder/validators", registration)
            .await?;
        let result = api_error_or_ok(response).await?;
        Ok(result)
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &mut BidRequest,
    ) -> Result<SignedBuilderBid, Error> {
        let target = format!(
            "/eth/v1/builder/header/{}/{}/{}",
            bid_request.slot, bid_request.parent_hash, bid_request.public_key
        );
        let response: VersionedValue<SignedBuilderBid> = self.api.get(&target).await?;
        Ok(response.data)
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let response: VersionedValue<ExecutionPayload> = self
            .api
            .post("/eth/v1/builder/blinded_blocks", signed_block)
            .await?;
        Ok(response.data)
    }
}
