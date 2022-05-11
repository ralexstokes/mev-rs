pub(crate) use ethereum_consensus::bellatrix::mainnet::{
    ExecutionPayload, ExecutionPayloadHeader, SignedBlindedBeaconBlock,
};
pub(crate) use ethereum_consensus::builder::SignedValidatorRegistration;
use ethereum_consensus::primitives::{BlsPublicKey, BlsSignature};
pub(crate) use ethereum_consensus::primitives::{Hash32, Slot};
use ssz_rs::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub struct BidRequest {
    #[serde(with = "crate::serde::as_string")]
    pub slot: Slot,
    pub parent_hash: Hash32,
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct BuilderBid {
    pub header: ExecutionPayloadHeader,
    pub value: U256,
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SignedBuilderBid {
    pub message: BuilderBid,
    pub signature: BlsSignature,
}
