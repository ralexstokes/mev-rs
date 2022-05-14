use async_trait::async_trait;
use beacon_api_client::{api_error_or_ok, Client as BeaconApiClient, VersionedValue};
use mev_build_rs::{
    BidRequest, Builder, Error as BuilderError, ExecutionPayload, SignedBlindedBeaconBlock,
    SignedBuilderBid, SignedValidatorRegistration,
};

pub type Error = beacon_api_client::Error;

pub struct Client {
    api: BeaconApiClient,
}

impl Client {
    pub fn new(api_client: BeaconApiClient) -> Self {
        Self { api: api_client }
    }

    pub async fn check_status(&self) -> Result<(), Error> {
        let response = self.api.http_get("/eth/v1/builder/status").await?;
        api_error_or_ok(response).await
    }
}

#[async_trait]
impl Builder for Client {
    async fn register_validator(
        &self,
        registration: &SignedValidatorRegistration,
    ) -> Result<(), BuilderError> {
        let response = self
            .api
            .http_post("/eth/v1/builder/validators", registration)
            .await?;
        let result = api_error_or_ok(response).await?;
        Ok(result)
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, BuilderError> {
        let target = format!(
            "/eth/v1/builder/header/{}/{}/{}",
            bid_request.slot, bid_request.parent_hash, bid_request.public_key
        );
        let response: VersionedValue<SignedBuilderBid> = self.api.get(&target).await?;
        Ok(response.data)
    }

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BuilderError> {
        let response: VersionedValue<ExecutionPayload> = self
            .api
            .post("/eth/v1/builder/blinded_blocks", signed_block)
            .await?;
        Ok(response.data)
    }
}
