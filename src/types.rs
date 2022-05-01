use ethereum_consensus::altair::mainnet::SyncAggregate;
pub use ethereum_consensus::bellatrix::mainnet::{ExecutionPayload, ExecutionPayloadHeader};
use ethereum_consensus::phase0::mainnet::{
    Attestation, AttesterSlashing, Deposit, Eth1Data, ProposerSlashing, SignedVoluntaryExit,
    MAX_ATTESTATIONS, MAX_ATTESTER_SLASHINGS, MAX_DEPOSITS, MAX_PROPOSER_SLASHINGS,
    MAX_VOLUNTARY_EXITS,
};
use ethereum_consensus::{
    crypto::{PublicKey as BLSPublicKey, Signature as BLSSignature},
    primitives::{Bytes32, ExecutionAddress, Hash32, Root, Slot, ValidatorIndex},
};
use ssz_rs::prelude::*;

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ValidatorRegistrationV1 {
    pub feeRecipient: ExecutionAddress,
    pub gasTarget: u64,
    pub timestamp: u64,
    pub pubkey: BLSPublicKey,
}

pub struct SignedValidatorRegistration {
    pub message: ValidatorRegistrationV1,
    pub signature: BLSSignature,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub struct BidRequest {
    pub slot: Slot,
    pub pubkey: BLSPublicKey,
    pub parentHash: Hash32,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct BuilderBidV1 {
    pub header: ExecutionPayloadHeader,
    pub value: U256,
    pub pubkey: BLSPublicKey,
}

pub struct SignedBuilderBid {
    pub message: BuilderBidV1,
    pub signature: BLSSignature,
}

#[derive(Debug)]
pub struct BlindedBeaconBlockBody {
    pub randao_reveal: BLSSignature,
    pub eth1_data: Eth1Data,
    pub graffiti: Bytes32,
    pub proposer_slashings: List<ProposerSlashing, MAX_PROPOSER_SLASHINGS>,
    pub attester_slashings: List<AttesterSlashing, MAX_ATTESTER_SLASHINGS>,
    pub attestations: List<Attestation, MAX_ATTESTATIONS>,
    pub deposits: List<Deposit, MAX_DEPOSITS>,
    pub voluntary_exits: List<SignedVoluntaryExit, MAX_VOLUNTARY_EXITS>,
    pub sync_aggregate: SyncAggregate,
    pub execution_payload_header: ExecutionPayloadHeader,
}

#[derive(Debug)]
pub struct BlindedBeaconBlock {
    pub slot: Slot,
    pub proposer_index: ValidatorIndex,
    pub parent_root: Root,
    pub state_root: Root,
    pub body: BlindedBeaconBlockBody,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SignedBlindedBeaconBlock {
    pub message: BlindedBeaconBlock,
    pub signature: BLSSignature,
}
