pub mod bellatrix;
pub mod capella;

use crate::signing::{verify_signed_builder_message, verify_signed_consensus_message};
pub use ethereum_consensus::builder::SignedValidatorRegistration;
use ethereum_consensus::{
    primitives::{BlsPublicKey, Hash32, Slot, ValidatorIndex},
    state_transition::{Context, Error},
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

#[derive(Debug)]
pub enum BuilderBid {
    Bellatrix(bellatrix::BuilderBid),
    Capella(capella::BuilderBid),
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "version", content = "data"))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum SignedBuilderBid {
    Bellatrix(bellatrix::SignedBuilderBid),
    Capella(capella::SignedBuilderBid),
}

impl SignedBuilderBid {
    pub fn value(&self) -> &U256 {
        match self {
            Self::Bellatrix(bid) => &bid.message.value,
            Self::Capella(bid) => &bid.message.value,
        }
    }

    pub fn block_hash(&self) -> &Hash32 {
        match self {
            Self::Bellatrix(bid) => &bid.message.header.block_hash,
            Self::Capella(bid) => &bid.message.header.block_hash,
        }
    }

    pub fn parent_hash(&self) -> &Hash32 {
        match self {
            Self::Bellatrix(bid) => &bid.message.header.parent_hash,
            Self::Capella(bid) => &bid.message.header.parent_hash,
        }
    }

    pub fn verify_signature(&mut self, context: &Context) -> Result<(), Error> {
        match self {
            Self::Bellatrix(bid) => {
                let public_key = bid.message.public_key.clone();
                verify_signed_builder_message(
                    &mut bid.message,
                    &bid.signature,
                    &public_key,
                    context,
                )
            }
            Self::Capella(bid) => {
                let public_key = bid.message.public_key.clone();
                verify_signed_builder_message(
                    &mut bid.message,
                    &bid.signature,
                    &public_key,
                    context,
                )
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "version", content = "data"))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum SignedBlindedBeaconBlock {
    Bellatrix(bellatrix::SignedBlindedBeaconBlock),
    Capella(capella::SignedBlindedBeaconBlock),
}

impl SignedBlindedBeaconBlock {
    pub fn slot(&self) -> Slot {
        match self {
            Self::Bellatrix(block) => block.message.slot,
            Self::Capella(block) => block.message.slot,
        }
    }

    pub fn proposer_index(&self) -> ValidatorIndex {
        match self {
            Self::Bellatrix(block) => block.message.proposer_index,
            Self::Capella(block) => block.message.proposer_index,
        }
    }

    pub fn block_hash(&self) -> &Hash32 {
        match self {
            Self::Bellatrix(block) => &block.message.body.execution_payload_header.block_hash,
            Self::Capella(block) => &block.message.body.execution_payload_header.block_hash,
        }
    }

    pub fn parent_hash(&self) -> &Hash32 {
        match self {
            Self::Bellatrix(block) => &block.message.body.execution_payload_header.parent_hash,
            Self::Capella(block) => &block.message.body.execution_payload_header.parent_hash,
        }
    }

    pub fn verify_signature(
        &mut self,
        public_key: &BlsPublicKey,
        context: &Context,
    ) -> Result<(), Error> {
        match self {
            Self::Bellatrix(block) => {
                let slot = block.message.slot;
                verify_signed_consensus_message(
                    &mut block.message,
                    &block.signature,
                    public_key,
                    context,
                    Some(slot),
                )
            }
            Self::Capella(block) => {
                let slot = block.message.slot;
                verify_signed_consensus_message(
                    &mut block.message,
                    &block.signature,
                    public_key,
                    context,
                    Some(slot),
                )
            }
        }
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(tag = "version", content = "data"))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum ExecutionPayload {
    Bellatrix(bellatrix::ExecutionPayload),
    Capella(capella::ExecutionPayload),
}

impl ExecutionPayload {
    pub fn block_hash(&self) -> &Hash32 {
        match self {
            Self::Bellatrix(payload) => &payload.block_hash,
            Self::Capella(payload) => &payload.block_hash,
        }
    }

    pub fn gas_limit(&self) -> u64 {
        match self {
            Self::Bellatrix(payload) => payload.gas_limit,
            Self::Capella(payload) => payload.gas_limit,
        }
    }
}

impl TryFrom<&mut ExecutionPayload> for ExecutionPayloadHeader {
    type Error = Error;

    fn try_from(value: &mut ExecutionPayload) -> Result<Self, Self::Error> {
        match value {
            ExecutionPayload::Bellatrix(payload) => {
                let header = bellatrix::ExecutionPayloadHeader::try_from(payload)?;
                Ok(Self::Bellatrix(header))
            }
            ExecutionPayload::Capella(payload) => {
                let header = capella::ExecutionPayloadHeader::try_from(payload)?;
                Ok(Self::Capella(header))
            }
        }
    }
}

#[derive(Debug)]
pub enum ExecutionPayloadHeader {
    Bellatrix(bellatrix::ExecutionPayloadHeader),
    Capella(capella::ExecutionPayloadHeader),
}

impl ExecutionPayloadHeader {
    pub fn block_hash(&self) -> &Hash32 {
        match self {
            Self::Bellatrix(header) => &header.block_hash,
            Self::Capella(header) => &header.block_hash,
        }
    }
}
