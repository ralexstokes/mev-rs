use crate::{
    signing::{compute_builder_signing_root, verify_signature},
    types::ExecutionPayload,
};
use ethereum_consensus::{
    primitives::{BlsPublicKey, BlsSignature, ExecutionAddress, Hash32, Slot, U256},
    ssz::prelude::*,
    state_transition::Context,
    Error,
};

#[derive(Debug, Default, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BidTrace {
    #[serde(with = "crate::serde::as_str")]
    pub slot: Slot,
    pub parent_hash: Hash32,
    pub block_hash: Hash32,
    #[serde(rename = "builder_pubkey")]
    pub builder_public_key: BlsPublicKey,
    #[serde(rename = "proposer_pubkey")]
    pub proposer_public_key: BlsPublicKey,
    pub proposer_fee_recipient: ExecutionAddress,
    #[serde(with = "crate::serde::as_str")]
    pub gas_limit: u64,
    #[serde(with = "crate::serde::as_str")]
    pub gas_used: u64,
    #[serde(with = "crate::serde::as_str")]
    pub value: U256,
}

#[derive(Debug, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignedBidSubmission {
    pub message: BidTrace,
    pub execution_payload: ExecutionPayload,
    pub signature: BlsSignature,
}

impl SignedBidSubmission {
    pub fn verify_signature(&mut self, context: &Context) -> Result<(), Error> {
        let signing_root = compute_builder_signing_root(&mut self.message, context)?;
        let public_key = &self.message.builder_public_key;
        verify_signature(public_key, signing_root.as_ref(), &self.signature)
    }
}
