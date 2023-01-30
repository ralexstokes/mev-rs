pub mod bellatrix;
pub mod capella;

pub use ethereum_consensus::builder::SignedValidatorRegistration;
use ethereum_consensus::{
    bellatrix::mainnet as bellatrix_types,
    capella::mainnet as capella_types,
    primitives::{BlsPublicKey, Hash32, Slot},
};
use ssz_rs::prelude::*;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BidRequest {
    #[serde(with = "crate::serde::as_string")]
    pub slot: Slot,
    pub parent_hash: Hash32,
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SignedBuilderBid {
    Bellatrix(bellatrix::SignedBuilderBid),
    Capella(capella::SignedBuilderBid),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ExecutionPayload {
    Bellatrix(bellatrix_types::ExecutionPayload),
    Capella(capella_types::ExecutionPayload),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SignedBlindedBeaconBlock {
    Bellatrix(bellatrix_types::SignedBlindedBeaconBlock),
    Capella(capella_types::SignedBlindedBeaconBlock),
}
