use crate::{
    blinded_block_relayer::BlindedBlockRelayer,
    types::{ProposerSchedule, SignedBidSubmission},
    Error,
};
use beacon_api_client::{api_error_or_ok, mainnet::Client as BeaconApiClient, Error as ApiError};

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

    async fn submit_bid(
        &self,
        signed_submission: &SignedBidSubmission,
        with_cancellations: bool,
    ) -> Result<(), Error> {
        let path = format!("/relay/v1/builder/blocks");
        let target = self.api.endpoint.join(&path).map_err(ApiError::from)?;
        let mut request = self.api.http.post(target).json(signed_submission);
        if with_cancellations {
            request = request.query(&[("cancellations", "1")])
        };
        let response = request.send().await.map_err(ApiError::from)?;
        api_error_or_ok(response).await.map_err(From::from)
    }
}
