use ethereum_consensus::{
    capella::mainnet as spec,
    deneb::Blob,
    primitives::{Bytes32, ExecutionAddress},
    ssz::prelude::{self as ssz_rs, ByteList, ByteVector, List},
};
use mev_rs::types::{BlobsBundle, ExecutionPayload};
use reth::{
    primitives::{Address, Bloom, SealedBlock, B256},
    rpc::types::engine::BlobsBundleV1,
};

pub fn to_bytes32(value: B256) -> Bytes32 {
    Bytes32::try_from(value.as_ref()).unwrap()
}

fn to_bytes20(value: Address) -> ExecutionAddress {
    ExecutionAddress::try_from(value.as_ref()).unwrap()
}

fn to_byte_vector(value: Bloom) -> ByteVector<256> {
    ByteVector::<256>::try_from(value.as_ref()).unwrap()
}

pub fn to_execution_payload(value: &SealedBlock) -> ExecutionPayload {
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

pub fn to_blobs_bundle(bundle: BlobsBundleV1) -> BlobsBundle {
    let commitments: Vec<_> = bundle
        .commitments
        .into_iter()
        .map(|c| ByteVector::<48>::try_from(c.as_ref()).unwrap())
        .collect();
    let commitments = List::try_from(commitments).unwrap();
    let proofs: Vec<_> = bundle
        .proofs
        .into_iter()
        .map(|p| ByteVector::<48>::try_from(p.as_ref()).unwrap())
        .collect();
    let proofs = List::try_from(proofs).unwrap();
    let blobs: Vec<_> =
        bundle.blobs.into_iter().map(|b| Blob::try_from(b.as_ref()).unwrap()).collect();
    let blobs = List::try_from(blobs).unwrap();
    BlobsBundle { commitments, proofs, blobs }
}
