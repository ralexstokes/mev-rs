use crate::types::ExecutionPayload;
use ethereum_consensus::{
    deneb::{
        mainnet::{Blob, MAX_BLOB_COMMITMENTS_PER_BLOCK},
        polynomial_commitments::{KzgCommitment, KzgProof},
    },
    ssz::prelude::*,
};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlobsBundle {
    commitments: List<KzgCommitment, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
    proofs: List<KzgProof, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
    blobs: List<Blob, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AuctionContents {
    pub execution_payload: ExecutionPayload,
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub blobs_bundle: Option<BlobsBundle>,
}
