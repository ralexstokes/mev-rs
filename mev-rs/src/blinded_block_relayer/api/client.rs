use crate::{
    blinded_block_relayer::BlindedBlockRelayer,
    types::{ProposerSchedule, SignedBidReceipt, SignedBidSubmission},
    Error,
};
use beacon_api_client::{mainnet::Client as BeaconApiClient, ApiResult, Error as ApiError};

/// A `Client` for a service implementing the Relay APIs.
#[derive(Clone)]
pub struct Client {
    api: BeaconApiClient,
}

impl Client {
    pub fn new(api_client: BeaconApiClient) -> Self {
        Self { api: api_client }
    }
}

#[async_trait::async_trait]
impl BlindedBlockRelayer for Client {
    async fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error> {
        self.api.get("/relay/v1/builder/validators").await.map_err(From::from)
    }

    // TODO support content types
    async fn submit_bid(
        &self,
        signed_submission: &SignedBidSubmission,
        cancellation_enabled: bool,
    ) -> Result<SignedBidReceipt, Error> {
        let response = self.api.http_post("/relay/v1/builder/blocks", signed_submission).await?;
        let receipt: ApiResult<SignedBidReceipt> = response.json().await.map_err(ApiError::from)?;
        match receipt {
            ApiResult::Ok(receipt) => Ok(receipt),
            ApiResult::Err(err) => Err(ApiError::from(err).into()),
        }
    }
}
