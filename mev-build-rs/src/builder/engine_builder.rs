use crate::blinded_block_provider::Error as BlindedBlockProviderError;
use crate::builder::Error;
use crate::types::{BidRequest as PayloadRequest, ExecutionPayloadWithValue};
use ethereum_consensus::{
    bellatrix::mainnet::ExecutionPayload, builder::SignedValidatorRegistration, primitives::U256,
    ssz::ByteList,
};

#[derive(Debug, Clone, Default)]
pub struct EngineBuilder {}

impl EngineBuilder {
    pub async fn run(&mut self) {}

    pub fn get_payload_with_value(
        &self,
        request: &PayloadRequest,
    ) -> Result<ExecutionPayloadWithValue, Error> {
        // TODO fetch correct fee recip
        let payload = ExecutionPayload {
            parent_hash: request.parent_hash.clone(),
            extra_data: ByteList::try_from(b"hello world".as_ref()).unwrap(),
            ..Default::default()
        };

        let bid = ExecutionPayloadWithValue {
            payload,
            value: U256::from_bytes_le([1u8; 32]),
        };
        Ok(bid)
    }

    pub async fn register_validators(
        &self,
        _registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        // collate registrations
        Ok(())
    }
}
