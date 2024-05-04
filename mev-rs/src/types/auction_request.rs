use ethereum_consensus::primitives::{BlsPublicKey, Hash32, Slot};

/// Describes a single unique auction.
#[derive(Debug, Default, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AuctionRequest {
    /// Slot for the proposal
    #[serde(with = "crate::serde::as_str")]
    pub slot: Slot,
    /// Hash of the parent block for the proposal
    pub parent_hash: Hash32,
    /// Public key of the proposer for the proposal
    pub public_key: BlsPublicKey,
}

impl std::fmt::Display for AuctionRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slot = self.slot;
        let parent_hash = &self.parent_hash;
        let public_key = &self.public_key;
        write!(f, "slot {slot}, parent hash {parent_hash} and proposer {public_key}")
    }
}
