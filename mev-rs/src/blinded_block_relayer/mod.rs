#[cfg(feature = "relay-api")]
mod api;

#[cfg(feature = "relay-api")]
pub use {api::client::Client, api::server::Server};

use crate::{
    error::Error,
    types::{ProposerSchedule, SignedBidSubmission},
};
use async_trait::async_trait;

#[async_trait]
pub trait BlindedBlockRelayer {
    async fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error>;

    // TODO: support cancellations?
    async fn submit_bid(&self, signed_submission: &SignedBidSubmission) -> Result<(), Error>;
}
