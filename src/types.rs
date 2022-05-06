pub use ethereum_consensus::bellatrix::mainnet::{
    BlindedBeaconBlock, ExecutionPayload, ExecutionPayloadHeader, SignedBlindedBeaconBlock,
};
use ethereum_consensus::primitives::{BlsPublicKey, BlsSignature, ExecutionAddress, Hash32, Slot};
use ssz_rs::prelude::*;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ValidatorRegistrationV1 {
    pub feeRecipient: ExecutionAddress,
    #[serde(with = "crate::serde::as_string")]
    pub gasLimit: u64,
    #[serde(with = "crate::serde::as_string")]
    pub timestamp: u64,
    pub pubkey: BlsPublicKey,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SignedValidatorRegistration {
    pub message: ValidatorRegistrationV1,
    pub signature: BlsSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub struct BidRequest {
    #[serde(with = "crate::serde::as_string")]
    pub slot: Slot,
    pub pubkey: BlsPublicKey,
    pub parentHash: Hash32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BuilderBidV1 {
    pub header: ExecutionPayloadHeader,
    pub value: U256,
    pub pubkey: BlsPublicKey,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SignedBuilderBid {
    pub message: BuilderBidV1,
    pub signature: BlsSignature,
}
