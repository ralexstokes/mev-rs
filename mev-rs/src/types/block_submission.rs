use crate::types::{auction_contents::deneb::BlobsBundle, ExecutionPayload};
use ethereum_consensus::{
    primitives::{BlsPublicKey, BlsSignature, ExecutionAddress, Hash32, Slot},
    ssz::prelude::*,
    Fork,
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

pub mod data_api {
    use super::*;

    #[derive(Debug, Default, Clone)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct PayloadTrace {
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
        #[serde(with = "crate::serde::as_str")]
        pub block_number: u64,
        #[serde(rename = "num_tx")]
        #[serde(with = "crate::serde::as_str")]
        pub transaction_count: usize,
        // NOTE: non-standard field
        #[serde(rename = "num_blob")]
        #[serde(with = "crate::serde::as_str")]
        pub blob_count: usize,
    }

    #[derive(Debug, Default, Clone)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct SubmissionTrace {
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
        #[serde(with = "crate::serde::as_str")]
        pub block_number: u64,
        #[serde(rename = "num_tx")]
        #[serde(with = "crate::serde::as_str")]
        pub transaction_count: usize,
        // NOTE: non-standard field
        #[serde(rename = "num_blob")]
        #[serde(with = "crate::serde::as_str")]
        pub blob_count: usize,
        #[serde(with = "crate::serde::as_str")]
        pub timestamp: u64,
        #[serde(with = "crate::serde::as_str")]
        pub timestamp_ms: u64,
    }
}

pub mod bellatrix {
    use super::{BidTrace, BlsSignature, ExecutionPayload};
    use ethereum_consensus::ssz::prelude::*;

    #[derive(Debug, Clone, Serializable, HashTreeRoot)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct SignedBidSubmission {
        pub message: BidTrace,
        pub execution_payload: ExecutionPayload,
        pub signature: BlsSignature,
    }
}

pub mod capella {
    pub use super::bellatrix::*;
}

pub mod deneb {
    use super::{BidTrace, BlsSignature, ExecutionPayload};
    use crate::types::auction_contents::deneb::BlobsBundle;
    use ethereum_consensus::ssz::prelude::*;

    #[derive(Debug, Clone, Serializable, HashTreeRoot)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct SignedBidSubmission {
        pub message: BidTrace,
        pub execution_payload: ExecutionPayload,
        pub blobs_bundle: BlobsBundle,
        pub signature: BlsSignature,
    }
}

#[derive(Debug, Clone, Serializable, HashTreeRoot)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[serde(untagged)]
#[ssz(transparent)]
pub enum SignedBidSubmission {
    Bellatrix(bellatrix::SignedBidSubmission),
    Capella(capella::SignedBidSubmission),
    Deneb(deneb::SignedBidSubmission),
}

impl<'de> serde::Deserialize<'de> for SignedBidSubmission {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        if let Ok(inner) = <_ as serde::Deserialize>::deserialize(&value) {
            return Ok(Self::Deneb(inner))
        }
        if let Ok(inner) = <_ as serde::Deserialize>::deserialize(&value) {
            return Ok(Self::Capella(inner))
        }
        if let Ok(inner) = <_ as serde::Deserialize>::deserialize(&value) {
            return Ok(Self::Bellatrix(inner))
        }
        Err(serde::de::Error::custom("no variant could be deserialized from input"))
    }
}

impl SignedBidSubmission {
    pub fn version(&self) -> Fork {
        match self {
            Self::Bellatrix(..) => Fork::Bellatrix,
            Self::Capella(..) => Fork::Capella,
            Self::Deneb(..) => Fork::Deneb,
        }
    }

    pub fn message(&self) -> &BidTrace {
        match self {
            Self::Bellatrix(inner) => &inner.message,
            Self::Capella(inner) => &inner.message,
            Self::Deneb(inner) => &inner.message,
        }
    }

    pub fn payload(&self) -> &ExecutionPayload {
        match self {
            Self::Bellatrix(inner) => &inner.execution_payload,
            Self::Capella(inner) => &inner.execution_payload,
            Self::Deneb(inner) => &inner.execution_payload,
        }
    }

    pub fn signature(&self) -> &BlsSignature {
        match self {
            Self::Bellatrix(inner) => &inner.signature,
            Self::Capella(inner) => &inner.signature,
            Self::Deneb(inner) => &inner.signature,
        }
    }

    pub fn blobs_bundle(&self) -> Option<&BlobsBundle> {
        match self {
            Self::Bellatrix(..) => None,
            Self::Capella(..) => None,
            Self::Deneb(inner) => Some(&inner.blobs_bundle),
        }
    }
}
