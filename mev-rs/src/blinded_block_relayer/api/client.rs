use crate::{
    blinded_block_relayer::BlindedBlockRelayer,
    types::{ProposerSchedule, SignedBidSubmission},
    Error,
};
use beacon_api_client::api_error_or_ok;

#[cfg(not(feature = "minimal-preset"))]
use beacon_api_client::mainnet::Client as BeaconApiClient;
#[cfg(feature = "minimal-preset")]
use beacon_api_client::minimal::Client as BeaconApiClient;

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

    async fn submit_bid(&self, signed_submission: &SignedBidSubmission) -> Result<(), Error> {
        let response = self.api.http_post("/relay/v1/builder/blocks", signed_submission).await?;
        api_error_or_ok(response).await.map_err(From::from)
    }
}
