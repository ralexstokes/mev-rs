use ethereum_consensus::{
    capella::mainnet as spec,
    primitives::{Bytes32, ExecutionAddress},
    ssz::{
        prelude as ssz_rs,
        prelude::{ByteList, ByteVector},
    },
};
use mev_rs::types::{
    bellatrix, capella,
    deneb::{self, BlobsBundle},
    ExecutionPayload,
};
use reth_primitives::{Bloom, SealedBlock, H160, H256, U256};

use reth_transaction_pool::TransactionPool;

use ssz_rs::List;

pub(crate) fn to_bytes32(value: H256) -> Bytes32 {
    Bytes32::try_from(value.as_bytes()).unwrap()
}

fn to_bytes20(value: H160) -> ExecutionAddress {
    ExecutionAddress::try_from(value.as_bytes()).unwrap()
}

fn to_byte_vector(value: Bloom) -> ByteVector<256> {
    ByteVector::<256>::try_from(value.as_bytes()).unwrap()
}

pub(crate) fn to_u256(value: &U256) -> ssz_rs::U256 {
    ssz_rs::U256::try_from_bytes_le(&value.to_le_bytes::<32>()).unwrap()
}

pub(crate) fn to_execution_payload<Pool>(value: &SealedBlock, pool: &Pool) -> ExecutionPayload
where
    Pool: TransactionPool,
{
    let hash = value.hash();
    let header = &value.header;
    let transactions = &value.body;
    let withdrawals = &value.withdrawals;
    let transactions = transactions
        .iter()
        .map(|t| spec::Transaction::try_from(t.envelope_encoded().as_ref()).unwrap())
        .collect::<Vec<_>>();

    let bellatrix_payload = bellatrix::ExecutionPayload {
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
        transactions: TryFrom::try_from(transactions.clone()).unwrap(),
    };

    if value.withdrawals.is_none() {
        return ExecutionPayload::Bellatrix(bellatrix_payload)
    }

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

    let capella_payload = capella::ExecutionPayload {
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
        transactions: TryFrom::try_from(transactions.clone()).unwrap(),
        withdrawals: TryFrom::try_from(withdrawals.clone()).unwrap(),
    };

    if header.blob_gas_used.is_none() && header.excess_blob_gas.is_none() {
        return ExecutionPayload::Capella(capella_payload)
    }

    let deneb_payload = deneb::ExecutionPayload {
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
        withdrawals: TryFrom::try_from(withdrawals.clone()).unwrap(),
        blob_gas_used: header.blob_gas_used.unwrap(),
        excess_blob_gas: header.excess_blob_gas.unwrap(),
    };

    let blob_tx_hashes = value.blob_transactions().iter().map(|t| t.hash()).collect::<Vec<_>>();
    let blobs_bundle = get_blob_bundles(pool, blob_tx_hashes);
    let deneb_payload_bundles =
        deneb::ExecutionPayloadAndBlobsBundle { execution_payload: deneb_payload, blobs_bundle };

    ExecutionPayload::Deneb(deneb_payload_bundles)
}

fn get_blob_bundles<Pool>(pool: &Pool, tx_hashes: Vec<H256>) -> BlobsBundle
where
    Pool: TransactionPool,
{
    let sidecars = pool.get_all_blobs_exact(tx_hashes).unwrap();

    let mut commitments = List::default();
    let mut proofs = List::default();
    let mut blobs = List::default();

    for sidecar in sidecars {
        for commitment in sidecar.commitments {
            let mut bytevector = ByteVector::<48>::default();
            for i in 0..48 {
                bytevector[i] = commitment[i];
            }
            commitments.push(bytevector);
        }

        for proof in sidecar.proofs {
            let mut bytevector = ByteVector::<48>::default();
            for i in 0..48 {
                bytevector[i] = proof[i];
            }
            proofs.push(bytevector);
        }

        for blob in sidecar.blobs {
            let mut bytevector = ByteVector::<131072>::default();
            for i in 0..131072 {
                bytevector[i] = blob[i];
            }
            blobs.push(bytevector);
        }
    }

    BlobsBundle { commitments, proofs, blobs }
}
