use ethereum_consensus::{
    bellatrix::mainnet::{
        Transaction, BYTES_PER_LOGS_BLOOM, MAX_EXTRA_DATA_BYTES, MAX_TRANSACTIONS_PER_PAYLOAD,
    },
    deneb::mainnet::{Blob, MAX_BLOBS_PER_BLOCK},
    kzg::{KzgCommitment, KzgProof},
    primitives::{Bytes32, ExecutionAddress, Hash32},
    ssz::{ByteList, ByteVector},
};
use serde::{Deserialize, Serialize};
use ssz_rs::prelude::*;

pub type PayloadId = ByteVector<8>;

pub fn u64_from_hex<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let data = <String>::deserialize(deserializer)?;
    let data = data.strip_prefix("0x").unwrap_or(data.as_ref());
    let value = u64::from_str_radix(data, 16).unwrap();
    Ok(value)
}

pub fn u256_from_be_hex<'de, D>(deserializer: D) -> Result<U256, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = <String>::deserialize(deserializer)?;
    let value = U256::from_hex(&s).unwrap();
    Ok(value)
}

// Quick hack to signal if we should use `engine_getPayloadV{1,2}`
// TODO improve this...
#[derive(Debug, Clone, Default)]
pub enum BuildVersion {
    #[default]
    V1,
    V2,
}

// `BuildJob` uniquely describes a block building process on the local execution client.
#[derive(Debug, Clone)]
pub struct BuildJob {
    pub head_block_hash: Hash32,
    pub timestamp: u64,
    pub suggested_fee_recipient: ExecutionAddress,
    pub payload_id: PayloadId,
    pub version: BuildVersion,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct WithdrawalV1 {
    #[serde(deserialize_with = "u64_from_hex")]
    pub index: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub validator_index: u64,
    pub address: ExecutionAddress,
    #[serde(deserialize_with = "u64_from_hex")]
    pub amount: u64,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ForkchoiceStateV1 {
    pub head_block_hash: Hash32,
    pub safe_block_hash: Hash32,
    pub finalized_block_hash: Hash32,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct PayloadAttributesV1 {
    #[serde(deserialize_with = "u64_from_hex")]
    pub timestamp: u64,
    pub prev_randao: Hash32,
    pub suggested_fee_recipient: ExecutionAddress,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct PayloadAttributesV2 {
    #[serde(deserialize_with = "u64_from_hex")]
    pub timestamp: u64,
    pub prev_randao: Hash32,
    pub suggested_fee_recipient: ExecutionAddress,
    // TODO: add bound on vec here?
    pub withdrawals: Vec<WithdrawalV1>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkchoiceUpdatedV1Params {
    pub forkchoice_state: ForkchoiceStateV1,
    pub payload_attributes: Option<PayloadAttributesV1>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PayloadAttributes {
    V1(PayloadAttributesV1),
    V2(PayloadAttributesV2),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkchoiceUpdatedV2Params {
    pub forkchoice_state: ForkchoiceStateV1,
    pub payload_attributes: Option<PayloadAttributes>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PayloadStatus {
    Valid,
    Invalid,
    Syncing,
    Accepted,
    InvalidBlockHash,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PayloadStatusV1 {
    pub status: PayloadStatus,
    pub latest_valid_hash: Option<Hash32>,
    pub validation_error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ForkchoiceUpdatedV1Response {
    pub payload_status: PayloadStatusV1,
    pub payload_id: Option<PayloadId>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum ExecutionPayload {
    V1(ExecutionPayloadV1),
    V2(ExecutionPayloadV2),
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
// TODO: maybe rename this to `GetPayloadV2Response` for consistency with the V3 response type?
pub struct ExecutionPayloadWithValue {
    pub execution_payload: ExecutionPayload,
    #[serde(deserialize_with = "u256_from_be_hex")]
    pub block_value: U256,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct GetPayloadV3Response {
    pub execution_payload: ExecutionPayloadV3,
    #[serde(deserialize_with = "u256_from_be_hex")]
    pub block_value: U256,
    pub blobs_bundle: BlobsBundleV1,
    pub should_override_builder: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct ExecutionPayloadV1 {
    pub parent_hash: Hash32,
    pub fee_recipient: ExecutionAddress,
    pub state_root: Bytes32,
    pub receipts_root: Bytes32,
    pub logs_bloom: ByteVector<BYTES_PER_LOGS_BLOOM>,
    pub prev_randao: Bytes32,
    #[serde(deserialize_with = "u64_from_hex")]
    pub block_number: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub gas_limit: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub gas_used: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub timestamp: u64,
    pub extra_data: ByteList<MAX_EXTRA_DATA_BYTES>,
    #[serde(deserialize_with = "u256_from_be_hex")]
    pub base_fee_per_gas: U256,
    pub block_hash: Hash32,
    pub transactions: List<Transaction, MAX_TRANSACTIONS_PER_PAYLOAD>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPayloadV2 {
    pub parent_hash: Hash32,
    pub fee_recipient: ExecutionAddress,
    pub state_root: Bytes32,
    pub receipts_root: Bytes32,
    pub logs_bloom: ByteVector<BYTES_PER_LOGS_BLOOM>,
    pub prev_randao: Bytes32,
    #[serde(deserialize_with = "u64_from_hex")]
    pub block_number: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub gas_limit: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub gas_used: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub timestamp: u64,
    pub extra_data: ByteList<MAX_EXTRA_DATA_BYTES>,
    #[serde(deserialize_with = "u256_from_be_hex")]
    pub base_fee_per_gas: U256,
    pub block_hash: Hash32,
    pub transactions: List<Transaction, MAX_TRANSACTIONS_PER_PAYLOAD>,
    // TODO: add bound on vec here?
    pub withdrawals: Vec<WithdrawalV1>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionPayloadV3 {
    pub parent_hash: Hash32,
    pub fee_recipient: ExecutionAddress,
    pub state_root: Bytes32,
    pub receipts_root: Bytes32,
    pub logs_bloom: ByteVector<BYTES_PER_LOGS_BLOOM>,
    pub prev_randao: Bytes32,
    #[serde(deserialize_with = "u64_from_hex")]
    pub block_number: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub gas_limit: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub gas_used: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub timestamp: u64,
    pub extra_data: ByteList<MAX_EXTRA_DATA_BYTES>,
    #[serde(deserialize_with = "u256_from_be_hex")]
    pub base_fee_per_gas: U256,
    pub block_hash: Hash32,
    pub transactions: List<Transaction, MAX_TRANSACTIONS_PER_PAYLOAD>,
    pub withdrawals: Vec<WithdrawalV1>,
    #[serde(deserialize_with = "u64_from_hex")]
    pub data_gas_used: u64,
    #[serde(deserialize_with = "u64_from_hex")]
    pub excess_data_gas: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlobsBundleV1 {
    pub commitments: List<KzgCommitment, MAX_BLOBS_PER_BLOCK>,
    pub proofs: List<KzgProof, MAX_BLOBS_PER_BLOCK>,
    pub blobs: List<Blob, MAX_BLOBS_PER_BLOCK>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_execution_payload_v1() {
        let json = r#"{"parentHash":"0xd6cf483bb88f7dc8037d491b29b0d2a29c8fb83dbaf9edf10a44bfd715eddbe1","feeRecipient":"0xf97e180c050e5ab072211ad2c213eb5aee4df134","stateRoot":"0x4941344e792c80d2073120b975923af67f7e484c29515c5a26d1a3df7b7e9b6b","receiptsRoot":"0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421","logsBloom":"0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000","prevRandao":"0xa275b854d2ed9374f2ae1d538f35bbe17688b04a091f8697e0a95f83f18e8ffe","blockNumber":"0xfd5","gasLimit":"0x1c9c380","gasUsed":"0x0","timestamp":"0x63e52f22","extraData":"0x","baseFeePerGas":"0x7","blockHash":"0xcd126775c64e5a59607862101394b0ee2d1f77da645f5f31cf4161882e47ca1f","transactions":[]}"#;
        let payload: ExecutionPayloadV1 = serde_json::from_str(json).unwrap();
        dbg!(payload);
    }
}
