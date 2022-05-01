#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct ValidatorRegistrationV1 {
    pub a: i64,
    // feeRecipient: Bytes20,
    // timestamp: u64,
    // gasLimit: u64,
    // pubkey: BLSPubkey,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Deserialize, serde::Serialize)]
pub struct BidRequest {
    pub a: i64,
    // slot: Slot,
    // pubkey: BLSPubkey,
    // parentHash: Hash,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuilderBidV1 {
    pub value: u64,
    // header: ExecutionPayloadHeader,
    // value: U256,
    // pubkey: BLSPubkey,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct SignedBlindedBeaconBlock {
    pub a: i64,
    // message: BlindedBeaconBlock,
    // signature: BLSSignature,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct ExecutionPayload {
    pub a: i64,
    // ...
}
