use std::cmp::Ordering;

use beacon_api_client::ValidatorStatus;
use ethereum_consensus::{
    builder::SignedValidatorRegistration,
    primitives::BlsPublicKey,
    state_transition::{Context, Error as ConsensusError},
};

use crate::verify_signed_builder_message;

use super::validator_summary_provider::{Error as ValidatorsError, ValidatorSummaryProvider};
use thiserror::Error;

pub struct ValidatorRegistrar<'a> {
    validators: &'a ValidatorSummaryProvider,
    context: &'a Context,
}

impl<'a> ValidatorRegistrar<'a> {
    pub fn new(validators: &'a ValidatorSummaryProvider, context: &'a Context) -> Self {
        ValidatorRegistrar { validators, context }
    }

    pub fn validate_registration(
        &self,
        registration: &mut SignedValidatorRegistration,
        current_timestamp: u64,
        latest_timestamp: Option<u64>,
    ) -> Result<ValidatorRegistrationStatus, Error> {
        let message = &mut registration.message;

        validate_registration_is_not_from_future(message.timestamp, current_timestamp)?;

        let registration_status = if let Some(latest_timestamp) = latest_timestamp {
            let status =
                determine_validator_registration_status(message.timestamp, latest_timestamp);
            if matches!(status, ValidatorRegistrationStatus::Outdated) {
                return Err(Error::InvalidTimestamp)
            }
            status
        } else {
            ValidatorRegistrationStatus::New
        };

        let validator_status = self.validators.get_status(&message.public_key)?;
        validate_validator_status(validator_status, &message.public_key)?;

        let public_key = message.public_key.clone();
        verify_signed_builder_message(message, &registration.signature, &public_key, self.context)?;

        Ok(registration_status)
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid timestamp")]
    InvalidTimestamp,
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error("validator {0} had an invalid status {1}")]
    InactiveValidator(BlsPublicKey, ValidatorStatus),
    #[error("{0}")]
    Validators(#[from] ValidatorsError),
}

pub enum ValidatorRegistrationStatus {
    New,
    Existing,
    Outdated,
}

fn validate_registration_is_not_from_future(
    timestamp: u64,
    current_timestamp: u64,
) -> Result<(), Error> {
    if timestamp > current_timestamp + 10 {
        Err(Error::InvalidTimestamp)
    } else {
        Ok(())
    }
}

fn determine_validator_registration_status(
    timestamp: u64,
    latest_timestamp: u64,
) -> ValidatorRegistrationStatus {
    match timestamp.cmp(&latest_timestamp) {
        Ordering::Less => ValidatorRegistrationStatus::Outdated,
        Ordering::Equal => ValidatorRegistrationStatus::Existing,
        Ordering::Greater => ValidatorRegistrationStatus::New,
    }
}

fn validate_validator_status(
    status: ValidatorStatus,
    public_key: &BlsPublicKey,
) -> Result<(), Error> {
    if matches!(status, ValidatorStatus::Active | ValidatorStatus::Pending) {
        Ok(())
    } else {
        Err(Error::InactiveValidator(public_key.clone(), status))
    }
}
