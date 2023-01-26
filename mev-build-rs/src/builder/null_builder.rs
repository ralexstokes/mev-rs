use crate::blinded_block_provider::Error as BlindedBlockProviderError;
use crate::types::BidRequest as PayloadRequest;
use ethereum_consensus::{
    bellatrix::mainnet::ExecutionPayload, builder::SignedValidatorRegistration, primitives::Bytes32,
};

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Config {
    extra_data: Bytes32,
}

impl Default for Config {
    fn default() -> Self {
        let extra_data = Bytes32::try_from(b"hello world hello world hello wo".as_ref()).unwrap();
        Self { extra_data }
    }
}

pub struct NullBuilder {}

impl NullBuilder {
    pub async fn produce_payload(&self, request: &PayloadRequest) -> ExecutionPayload {
        ExecutionPayload::default()
    }

    pub async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        // collate registrations
        Ok(())
    }
}
