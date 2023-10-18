use ethereum_consensus::primitives::{BlsPublicKey, Hash32, Slot};

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BidRequest {
    #[serde(with = "crate::serde::as_str")]
    pub slot: Slot,
    pub parent_hash: Hash32,
    pub public_key: BlsPublicKey,
}

impl std::fmt::Display for BidRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let slot = self.slot;
        let parent_hash = &self.parent_hash;
        let public_key = &self.public_key;
        write!(f, "slot {slot}, parent hash {parent_hash} and proposer {public_key}")
    }
}
