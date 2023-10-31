use ethereum_consensus::{builder::SignedValidatorRegistration, primitives::ExecutionAddress};

#[derive(Debug, Hash, PartialEq, Eq)]
pub struct ValidatorPreferences {
    pub fee_recipient: ExecutionAddress,
    pub gas_limit: u64,
}

impl From<&SignedValidatorRegistration> for ValidatorPreferences {
    fn from(value: &SignedValidatorRegistration) -> Self {
        Self {
            fee_recipient: value.message.fee_recipient.clone(),
            gas_limit: value.message.gas_limit,
        }
    }
}
