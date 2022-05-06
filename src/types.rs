pub use ethereum_consensus::bellatrix::mainnet::{
    BlindedBeaconBlock, ExecutionPayload, ExecutionPayloadHeader, SignedBlindedBeaconBlock,
};
pub use ethereum_consensus::builder::SignedValidatorRegistration;
use ethereum_consensus::primitives::{BlsPublicKey, BlsSignature, Hash32, Slot};
use ssz_rs::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub struct BidRequest {
    #[serde(with = "crate::serde::as_string")]
    pub slot: Slot,
    pub public_key: BlsPublicKey,
    pub parent_hash: Hash32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BuilderBidV1 {
    pub header: ExecutionPayloadHeader,
    pub value: U256,
    pub public_key: BlsPublicKey,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SignedBuilderBid {
    pub message: BuilderBidV1,
    pub signature: BlsSignature,
}
