use ethereum_consensus::{
    capella::mainnet as spec,
    primitives::{Bytes32, ExecutionAddress},
    ssz::{
        prelude as ssz_rs,
        prelude::{ByteList, ByteVector},
    },
};
use mev_rs::types::ExecutionPayload;
use reth_primitives::{Address, Bloom, SealedBlock, B256, U256};

pub(crate) fn to_bytes32(value: B256) -> Bytes32 {
    Bytes32::try_from(value.as_ref()).unwrap()
}

fn to_bytes20(value: Address) -> ExecutionAddress {
    ExecutionAddress::try_from(value.as_ref()).unwrap()
}

fn to_byte_vector(value: Bloom) -> ByteVector<256> {
    ByteVector::<256>::try_from(value.as_ref()).unwrap()
}

pub(crate) fn to_u256(value: &U256) -> ssz_rs::U256 {
    *value
}

pub(crate) fn to_execution_payload(value: &SealedBlock) -> ExecutionPayload {
    let hash = value.hash();
    let header = &value.header;
    let transactions = &value.body;
    let withdrawals = &value.withdrawals;
    let transactions = transactions
        .iter()
        .map(|t| spec::Transaction::try_from(t.envelope_encoded().as_ref()).unwrap())
        .collect::<Vec<_>>();
    let withdrawals = withdrawals
        .as_ref()
        .unwrap()
        .iter()
        .map(|w| spec::Withdrawal {
            index: w.index as usize,
            validator_index: w.validator_index as usize,
            address: to_bytes20(w.address),
            amount: w.amount,
        })
        .collect::<Vec<_>>();

    let payload = spec::ExecutionPayload {
        parent_hash: to_bytes32(header.parent_hash),
        fee_recipient: to_bytes20(header.beneficiary),
        state_root: to_bytes32(header.state_root),
        receipts_root: to_bytes32(header.receipts_root),
        logs_bloom: to_byte_vector(header.logs_bloom),
        prev_randao: to_bytes32(header.mix_hash),
        block_number: header.number,
        gas_limit: header.gas_limit,
        gas_used: header.gas_used,
        timestamp: header.timestamp,
        extra_data: ByteList::try_from(header.extra_data.as_ref()).unwrap(),
        base_fee_per_gas: ssz_rs::U256::from(header.base_fee_per_gas.unwrap_or_default()),
        block_hash: to_bytes32(hash),
        transactions: TryFrom::try_from(transactions).unwrap(),
        withdrawals: TryFrom::try_from(withdrawals).unwrap(),
    };
    ExecutionPayload::Capella(payload)
}
