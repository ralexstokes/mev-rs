use crate::{
    blinded_block_provider::Error,
    types::{
        BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
};
use axum::http::StatusCode;
use beacon_api_client::{
    api_error_or_ok, ApiResult, Client as BeaconApiClient, Error as BeaconApiError, VersionedValue,
};

/// A `Client` for a service implementing the Builder APIs.
/// Note that `Client` does not implement the `Builder` trait so that
/// it can provide more flexibility to callers with respect to the types
/// it accepts.
#[derive(Clone)]
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

    pub async fn register_validators(
        &self,
        registrations: &[SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let response = self.api.http_post("/eth/v1/builder/validators", &registrations).await?;
        api_error_or_ok(response).await.map_err(From::from)
    }

    pub async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, Error> {
        let target = format!(
            "/eth/v1/builder/header/{}/{}/{}",
            bid_request.slot, bid_request.parent_hash, bid_request.public_key
        );
        let response = self.api.http_get(&target).await?;

        if response.status() == StatusCode::NO_CONTENT {
            return Err(Error::NoBidPrepared(Box::new(bid_request.clone())))
        }

        let result: ApiResult<VersionedValue<SignedBuilderBid>> =
            response.json().await.map_err(beacon_api_client::Error::Http)?;
        match result {
            ApiResult::Ok(result) => Ok(result.payload),
            ApiResult::Err(err) => Err(err.into()),
        }
    }

    pub async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let response = self.api.http_post("/eth/v1/builder/blinded_blocks", signed_block).await?;

        let response: VersionedValue<ExecutionPayload> =
            response.json().await.map_err(|err| -> Error { BeaconApiError::Http(err).into() })?;
        Ok(response.payload)
    }
}
