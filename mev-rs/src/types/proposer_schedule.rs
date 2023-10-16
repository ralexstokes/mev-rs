use crate::types::SignedValidatorRegistration;
use ethereum_consensus::primitives::{Slot, ValidatorIndex};

#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProposerSchedule {
    #[serde(with = "crate::serde::as_string")]
    pub slot: Slot,
    #[serde(with = "crate::serde::as_string")]
    pub validator_index: ValidatorIndex,
    pub entry: SignedValidatorRegistration,
}
