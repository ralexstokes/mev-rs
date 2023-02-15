use ethereum_consensus::{
    bellatrix::mainnet as bellatrix,
    capella::mainnet as capella,
    primitives::{ExecutionAddress, U256},
    ssz::ByteList,
    state_transition::{Context, Forks},
};
use mev_rs::{
    types::{BidRequest as PayloadRequest, ExecutionPayload},
    Error,
};

// A `NullBuilder` builds empty blocks. Primarily used for testing.
// Currently, the blocks are not necessarily consensus-consistent...
pub struct NullBuilder;

impl NullBuilder {
    pub fn get_payload_with_value(
        &self,
        request: &PayloadRequest,
        fee_recipient: &ExecutionAddress,
        gas_limit: u64,
        context: &Context,
    ) -> Result<(ExecutionPayload, U256), Error> {
        let fork = context.fork_for(request.slot);
        let payload = match fork {
            Forks::Bellatrix => ExecutionPayload::Bellatrix(bellatrix::ExecutionPayload {
                parent_hash: request.parent_hash.clone(),
                fee_recipient: fee_recipient.clone(),
                gas_limit,
                extra_data: ByteList::try_from(b"hello world in bellatrix".as_ref()).unwrap(),
                ..Default::default()
            }),
            Forks::Capella => ExecutionPayload::Capella(capella::ExecutionPayload {
                parent_hash: request.parent_hash.clone(),
                fee_recipient: fee_recipient.clone(),
                gas_limit,
                extra_data: ByteList::try_from(b"hello world in capella".as_ref()).unwrap(),
                ..Default::default()
            }),
            _ => unimplemented!(),
        };

        Ok((payload, U256::zero()))
    }
}
