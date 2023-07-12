pub use ethereum_consensus::{builder::SignedValidatorRegistration, deneb::mainnet as spec};
use ethereum_consensus::{
    deneb::mainnet::MAX_BLOBS_PER_BLOCK,
    kzg::{KzgCommitment, KzgProof},
    primitives::{BlsPublicKey, BlsSignature, Root, U256},
};
use ssz_rs::prelude::*;

// NOTE: type alias here to call out the important types clearly, in lieu of just `pub use ...`
pub type ExecutionPayload = spec::ExecutionPayload;
pub type ExecutionPayloadHeader = spec::ExecutionPayloadHeader;
pub type SignedBlindedBeaconBlock = spec::SignedBlindedBeaconBlock;
pub type SignedBlindedBlobSidecar = spec::SignedBlindedBlobSidecar;

#[derive(Debug, Default, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BuilderBid {
    pub header: spec::ExecutionPayloadHeader,
    pub blinded_blobs_bundle: BlindedBlobsBundle,
    pub value: U256,
    #[serde(rename = "pubkey")]
    pub public_key: BlsPublicKey,
}

#[derive(Debug, Default, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BlindedBlobsBundle {
    pub commitments: List<KzgCommitment, MAX_BLOBS_PER_BLOCK>,
    pub proofs: List<KzgProof, MAX_BLOBS_PER_BLOCK>,
    pub blob_roots: List<Root, MAX_BLOBS_PER_BLOCK>,
}

#[derive(Debug, Default, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignedBuilderBid {
    pub message: BuilderBid,
    pub signature: BlsSignature,
}

#[derive(Debug, Default, Clone, SimpleSerialize)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SignedBlindedBlockAndBlobSidecars {
    pub signed_blinded_block: SignedBlindedBeaconBlock,
    pub signed_blinded_blob_sidecars: List<SignedBlindedBlobSidecar, MAX_BLOBS_PER_BLOCK>,
}
