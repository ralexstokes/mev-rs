use ethereum_consensus::primitives::{BlsPublicKey, BlsSignature, U256};
pub use ethereum_consensus::{
    builder::SignedValidatorRegistration,
    capella::mainnet::{ExecutionPayload, ExecutionPayloadHeader, SignedBlindedBeaconBlock},
};
use ssz_rs::prelude::*;

#[derive(Debug, Default, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuilderBid {
    pub header: ExecutionPayloadHeader,
    pub value: U256,
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Default, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignedBuilderBid {
    pub message: BuilderBid,
    pub signature: BlsSignature,
}
