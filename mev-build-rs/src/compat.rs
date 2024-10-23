use crate::Error;
use alloy::eips::eip2718::Encodable2718;
use ethereum_consensus::{
    crypto::{KzgCommitment, KzgProof},
    primitives::{Bytes32, ExecutionAddress},
    ssz::prelude::{ByteList, ByteVector, SimpleSerializeError, U256},
    Fork,
};
use mev_rs::types::{BlobsBundle, ExecutionPayload};
use reth::primitives::{
    revm_primitives::{alloy_primitives::Bloom, Address, B256},
    BlobTransactionSidecar, SealedBlock,
};

#[cfg(not(feature = "minimal-preset"))]
use ethereum_consensus::deneb::mainnet as deneb;
#[cfg(feature = "minimal-preset")]
use ethereum_consensus::deneb::minimal as deneb;

pub fn to_bytes32(value: B256) -> Bytes32 {
    Bytes32::try_from(value.as_ref()).unwrap()
}

pub fn to_bytes20(value: Address) -> ExecutionAddress {
    ExecutionAddress::try_from(value.as_ref()).unwrap()
}

fn to_byte_vector(value: Bloom) -> ByteVector<256> {
    ByteVector::<256>::try_from(value.as_ref()).unwrap()
}

pub fn to_execution_payload(value: &SealedBlock, fork: Fork) -> Result<ExecutionPayload, Error> {
    let hash = value.hash();
    let header = &value.header;
    let transactions = &value.body.transactions;
    let withdrawals = &value.body.withdrawals;
    match fork {
        Fork::Deneb => {
            let transactions = transactions
                .iter()
                .map(|t| deneb::Transaction::try_from(t.encoded_2718().as_ref()).unwrap())
                .collect::<Vec<_>>();
            let withdrawals = withdrawals
                .as_ref()
                .unwrap()
                .iter()
                .map(|w| deneb::Withdrawal {
                    index: w.index as usize,
                    validator_index: w.validator_index as usize,
                    address: to_bytes20(w.address),
                    amount: w.amount,
                })
                .collect::<Vec<_>>();

            let payload = deneb::ExecutionPayload {
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
                base_fee_per_gas: U256::from(header.base_fee_per_gas.unwrap_or_default()),
                block_hash: to_bytes32(hash),
                transactions: TryFrom::try_from(transactions).unwrap(),
                withdrawals: TryFrom::try_from(withdrawals).unwrap(),
                blob_gas_used: header.blob_gas_used.unwrap(),
                excess_blob_gas: header.excess_blob_gas.unwrap(),
            };
            Ok(ExecutionPayload::Deneb(payload))
        }
        fork => Err(Error::UnsupportedFork(fork)),
    }
}

pub fn to_blobs_bundle(sidecars: &[BlobTransactionSidecar]) -> Result<BlobsBundle, Error> {
    let mut commitments = vec![];
    let mut proofs = vec![];
    let mut blobs = vec![];

    for sidecar in sidecars {
        for commitment in &sidecar.commitments {
            let commitment = KzgCommitment::try_from(commitment.as_slice()).unwrap();
            commitments.push(commitment);
        }
        for proof in &sidecar.proofs {
            let proof = KzgProof::try_from(proof.as_slice()).unwrap();
            proofs.push(proof);
        }
        for blob in &sidecar.blobs {
            let blob = deneb::Blob::try_from(blob.as_ref()).unwrap();
            blobs.push(blob);
        }
    }

    Ok(BlobsBundle {
        commitments: commitments
            .try_into()
            .map_err(|(_, err): (_, SimpleSerializeError)| Error::Consensus(err.into()))?,
        proofs: proofs
            .try_into()
            .map_err(|(_, err): (_, SimpleSerializeError)| Error::Consensus(err.into()))?,

        blobs: blobs
            .try_into()
            .map_err(|(_, err): (_, SimpleSerializeError)| Error::Consensus(err.into()))?,
    })
}
