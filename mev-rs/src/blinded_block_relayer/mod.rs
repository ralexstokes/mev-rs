#[cfg(feature = "relay-api")]
mod api;

#[cfg(feature = "relay-api")]
pub use {api::client::Client, api::server::Server};

use crate::{
    error::Error,
    types::{
        block_submission::data_api::{PayloadTrace, SubmissionTrace},
        ProposerSchedule, SignedBidSubmission, SignedValidatorRegistration,
    },
};
use async_trait::async_trait;
use ethereum_consensus::primitives::{BlsPublicKey, Bytes32, Slot};

#[async_trait]
pub trait BlindedBlockRelayer {
    async fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error>;

    async fn submit_bid(&self, signed_submission: &SignedBidSubmission) -> Result<(), Error>;
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct DeliveredPayloadFilter {
    pub slot: Option<Slot>,
    pub block_hash: Option<Bytes32>,
    pub block_number: Option<usize>,
    #[serde(rename = "proposer_pubkey")]
    pub proposer_public_key: Option<BlsPublicKey>,
    #[serde(rename = "builder_pubkey")]
    pub builder_public_key: Option<BlsPublicKey>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct BlockSubmissionFilter {
    pub slot: Option<Slot>,
    pub block_hash: Option<Bytes32>,
    pub block_number: Option<usize>,
    #[serde(rename = "builder_pubkey")]
    pub builder_public_key: Option<BlsPublicKey>,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub struct ValidatorRegistrationQuery {
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}

#[async_trait]
pub trait BlindedBlockDataProvider {
    async fn get_delivered_payloads(
        &self,
        filters: &DeliveredPayloadFilter,
    ) -> Result<Vec<PayloadTrace>, Error>;

    async fn get_block_submissions(
        &self,
        filters: &BlockSubmissionFilter,
    ) -> Result<Vec<SubmissionTrace>, Error>;

    async fn fetch_validator_registration(
        &self,
        public_key: &BlsPublicKey,
    ) -> Result<SignedValidatorRegistration, Error>;
}
