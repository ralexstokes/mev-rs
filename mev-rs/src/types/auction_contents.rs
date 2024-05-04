use crate::types::ExecutionPayload;
use ethereum_consensus::Fork;

pub mod bellatrix {
    use super::ExecutionPayload;

    pub type AuctionContents = ExecutionPayload;
}

pub mod capella {
    pub use super::bellatrix::*;
}

pub mod deneb {
    use super::ExecutionPayload;
    use ethereum_consensus::{
        deneb::polynomial_commitments::{KzgCommitment, KzgProof},
        ssz::prelude::*,
    };

    #[cfg(not(feature = "minimal-preset"))]
    use ethereum_consensus::deneb::mainnet::{Blob, MAX_BLOB_COMMITMENTS_PER_BLOCK};
    #[cfg(feature = "minimal-preset")]
    use ethereum_consensus::deneb::minimal::{Blob, MAX_BLOB_COMMITMENTS_PER_BLOCK};

    #[derive(Clone, Debug, Default, Serializable, HashTreeRoot, PartialEq, Eq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct BlobsBundle {
        pub commitments: List<KzgCommitment, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
        pub proofs: List<KzgProof, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
        pub blobs: List<Blob, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
    }

    #[derive(Debug)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct AuctionContents {
        pub execution_payload: ExecutionPayload,
        pub blobs_bundle: BlobsBundle,
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[serde(untagged)]
pub enum AuctionContents {
    Bellatrix(bellatrix::AuctionContents),
    Capella(capella::AuctionContents),
    Deneb(deneb::AuctionContents),
}

impl<'de> serde::Deserialize<'de> for AuctionContents {
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

impl AuctionContents {
    pub fn version(&self) -> Fork {
        match self {
            Self::Bellatrix(..) => Fork::Bellatrix,
            Self::Capella(..) => Fork::Capella,
            Self::Deneb(..) => Fork::Deneb,
        }
    }

    pub fn execution_payload(&self) -> &ExecutionPayload {
        match self {
            Self::Bellatrix(inner) => inner,
            Self::Capella(inner) => inner,
            Self::Deneb(inner) => &inner.execution_payload,
        }
    }

    pub fn blobs_bundle(&self) -> Option<&deneb::BlobsBundle> {
        match self {
            Self::Deneb(inner) => Some(&inner.blobs_bundle),
            _ => None,
        }
    }
}
