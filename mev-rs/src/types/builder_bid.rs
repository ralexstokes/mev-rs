use crate::{
    signing::{compute_builder_signing_root, sign_builder_message, verify_signature, SecretKey},
    types::ExecutionPayloadHeader,
};
use ethereum_consensus::{
    primitives::{BlsPublicKey, BlsSignature, U256},
    ssz::prelude::*,
    state_transition::Context,
    Error, Fork,
};
use std::fmt;

#[derive(Debug, Clone, Merkleized)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuilderBid {
    pub header: ExecutionPayloadHeader,
    pub value: U256,
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}

impl BuilderBid {
    pub fn sign(
        mut self,
        secret_key: &SecretKey,
        context: &Context,
    ) -> Result<SignedBuilderBid, Error> {
        let signature = sign_builder_message(&mut self, secret_key, context)?;
        Ok(SignedBuilderBid { message: self, signature })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SignedBuilderBid {
    pub message: BuilderBid,
    pub signature: BlsSignature,
}

impl SignedBuilderBid {
    pub fn version(&self) -> Fork {
        self.message.header.version()
    }
}

impl fmt::Display for SignedBuilderBid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let block_hash = self.message.header.block_hash();
        let value = &self.message.value;
        write!(f, "block hash {block_hash} and value {value:?}")
    }
}

impl SignedBuilderBid {
    pub fn verify_signature(&mut self, context: &Context) -> Result<(), Error> {
        let signing_root = compute_builder_signing_root(&mut self.message, context)?;
        let public_key = &self.message.public_key;
        verify_signature(public_key, signing_root.as_ref(), &self.signature)
    }
}
