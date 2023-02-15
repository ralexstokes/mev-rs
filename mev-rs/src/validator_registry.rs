use crate::{signing::verify_signed_builder_message, types::SignedValidatorRegistration};
use beacon_api_client::{Client, Error as ApiError, StateId, ValidatorStatus, ValidatorSummary};
use ethereum_consensus::{
    builder::ValidatorRegistration,
    primitives::{BlsPublicKey, ExecutionAddress, ValidatorIndex},
    state_transition::{Context, Error as ConsensusError},
};
use parking_lot::Mutex;
use std::{cmp::Ordering, collections::HashMap};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid timestamp")]
    InvalidTimestamp,
    #[error("validator {0} had an invalid status {1}")]
    InactiveValidator(BlsPublicKey, ValidatorStatus),
    #[error("missing knowledge of pubkey in validator set")]
    UnknownPubkey,
    #[error("missing knowledge of index in validator set")]
    UnknownIndex,
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
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

enum ValidatorRegistrationStatus {
    New,
    Existing,
    Outdated,
}

fn validate_validator_status(
    status: ValidatorStatus,
    public_key: &BlsPublicKey,
) -> Result<(), Error> {
    if matches!(status, ValidatorStatus::Pending | ValidatorStatus::ActiveOngoing) {
        Ok(())
    } else {
        Err(Error::InactiveValidator(public_key.clone(), status))
    }
}

#[derive(Default, Debug)]
pub struct State {
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
    validators: HashMap<BlsPublicKey, ValidatorSummary>,
    pubkeys_by_index: HashMap<ValidatorIndex, BlsPublicKey>,
}

pub struct ValidatorRegistry {
    client: Client,
    state: Mutex<State>,
}

impl ValidatorRegistry {
    pub fn new(client: Client) -> Self {
        let state = State::default();
        Self { client, state: Mutex::new(state) }
    }

    pub async fn load(&self) -> Result<(), Error> {
        let summaries = self.client.get_validators(StateId::Head, &[], &[]).await?;
        let mut state = self.state.lock();
        for summary in summaries.into_iter() {
            let public_key = summary.validator.public_key.clone();
            state.pubkeys_by_index.insert(summary.index, public_key.clone());
            state.validators.insert(public_key, summary);
        }
        Ok(())
    }

    pub fn get_public_key(&self, index: ValidatorIndex) -> Result<BlsPublicKey, Error> {
        let state = self.state.lock();
        state.pubkeys_by_index.get(&index).cloned().ok_or(Error::UnknownIndex)
    }

    pub fn get_validator_index(&self, public_key: &BlsPublicKey) -> Option<ValidatorIndex> {
        let state = self.state.lock();
        state.validators.get(public_key).map(|v| v.index)
    }

    pub fn get_preferences(&self, public_key: &BlsPublicKey) -> Option<ValidatorRegistration> {
        let state = self.state.lock();
        state.validator_preferences.get(public_key).map(|registration| registration.message.clone())
    }

    pub fn find_public_key_by_fee_recipient(
        &self,
        fee_recipient: &ExecutionAddress,
    ) -> Option<BlsPublicKey> {
        let state = self.state.lock();
        state
            .validator_preferences
            .iter()
            .find(|&(_, preferences)| &preferences.message.fee_recipient == fee_recipient)
            .map(|(key, _)| key.clone())
    }

    pub fn validate_registrations(
        &self,
        registrations: &mut [SignedValidatorRegistration],
        current_timestamp: u64,
        context: &Context,
    ) -> Result<(), Error> {
        for registration in registrations.iter_mut() {
            // TODO one failure should not fail the others...
            self.validate_registration(registration, current_timestamp, context)?;
        }
        Ok(())
    }

    fn validate_registration(
        &self,
        registration: &mut SignedValidatorRegistration,
        current_timestamp: u64,
        context: &Context,
    ) -> Result<(), Error> {
        let mut state = self.state.lock();
        let latest_timestamp = state
            .validator_preferences
            .get(&registration.message.public_key)
            .map(|r| r.message.timestamp);
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

        let public_key = &message.public_key;
        let validator_status = state
            .validators
            .get(public_key)
            .map(|validator| validator.status)
            .ok_or(Error::UnknownPubkey)?;
        validate_validator_status(validator_status, public_key)?;

        let public_key = message.public_key.clone();
        verify_signed_builder_message(message, &registration.signature, &public_key, context)?;

        if matches!(registration_status, ValidatorRegistrationStatus::New) {
            let public_key = registration.message.public_key.clone();
            state.validator_preferences.insert(public_key, registration.clone());
        }
        Ok(())
    }
}
