use crate::{
    signing::{compute_builder_signing_root, verify_signature},
    types::SignedValidatorRegistration,
};
use beacon_api_client::{
    mainnet::Client, Error as ApiError, StateId, ValidatorStatus, ValidatorSummary,
};
use ethereum_consensus::{
    builder::ValidatorRegistration,
    primitives::{BlsPublicKey, Epoch, Slot, ValidatorIndex},
    state_transition::Context,
    Error as ConsensusError,
};
use parking_lot::RwLock;
use rayon::prelude::*;
use std::{cmp::Ordering, collections::HashMap};
use thiserror::Error;
use tracing::trace;

#[derive(Debug, Error)]
pub enum Error {
    #[error("local time is {1} but registration has timestamp from future: {0:?}")]
    FutureRegistration(ValidatorRegistration, u64),
    #[error("validator has registration from timestamp {1}; outdated registration: {0:?}")]
    OutdatedRegistration(ValidatorRegistration, u64),
    #[error("registration is for validator with invalid status {1}: {0:?}")]
    ValidatorStatus(ValidatorRegistration, ValidatorStatus),
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
    message: &ValidatorRegistration,
    current_timestamp: u64,
) -> Result<(), Error> {
    let timestamp = message.timestamp;
    if timestamp > current_timestamp + 10 {
        Err(Error::FutureRegistration(message.clone(), current_timestamp))
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
    message: &ValidatorRegistration,
    status: ValidatorStatus,
) -> Result<(), Error> {
    if matches!(status, ValidatorStatus::Pending | ValidatorStatus::ActiveOngoing) {
        Ok(())
    } else {
        Err(Error::ValidatorStatus(message.clone(), status))
    }
}

#[derive(Default, Debug)]
pub struct State {
    // data from registered validators
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
    // data from consensus
    validators: HashMap<BlsPublicKey, ValidatorSummary>,
    pubkeys_by_index: HashMap<ValidatorIndex, BlsPublicKey>,
}

// Maintains validators we are aware of
pub struct ValidatorRegistry {
    client: Client,
    slots_per_epoch: Slot,
    state: RwLock<State>,
}

impl ValidatorRegistry {
    pub fn new(client: Client, slots_per_epoch: Slot) -> Self {
        let state = RwLock::new(Default::default());
        Self { client, slots_per_epoch, state }
    }

    // TODO: load more efficiently
    pub async fn on_epoch(&self, epoch: Epoch) -> Result<(), Error> {
        let slot = epoch * self.slots_per_epoch;
        let summaries = self.client.get_validators(StateId::Slot(slot), &[], &[]).await?;
        let mut state = self.state.write();
        for summary in summaries.into_iter() {
            let public_key = summary.validator.public_key.clone();
            state.pubkeys_by_index.insert(summary.index, public_key.clone());
            state.validators.insert(public_key, summary);
        }
        Ok(())
    }

    // Return the BLS public key for the validator's `index`, reflecting the index
    // built from the last consensus update
    pub fn get_public_key(&self, index: ValidatorIndex) -> Option<BlsPublicKey> {
        let state = self.state.read();
        state.pubkeys_by_index.get(&index).cloned()
    }

    // pub fn get_validator_index(&self, public_key: &BlsPublicKey) -> Option<ValidatorIndex> {
    //     let state = self.state.read();
    //     state.validators.get(public_key).map(|v| v.index)
    // }

    // Return the signed validator registration for the given `public_key` if we have processed such
    // a registration. If missing, return `None`.
    pub fn get_signed_registration(
        &self,
        public_key: &BlsPublicKey,
    ) -> Option<SignedValidatorRegistration> {
        let state = self.state.read();
        state.validator_preferences.get(public_key).cloned()
    }

    // pub fn find_public_key_by_fee_recipient(
    //     &self,
    //     fee_recipient: &ExecutionAddress,
    // ) -> Option<BlsPublicKey> {
    //     let state = self.state.lock();
    //     state
    //         .validator_preferences
    //         .iter()
    //         .find(|&(_, preferences)| &preferences.message.fee_recipient == fee_recipient)
    //         .map(|(key, _)| key.clone())
    // }

    fn process_registration<'a>(
        &'a self,
        registration: &'a mut SignedValidatorRegistration,
        current_timestamp: u64,
        context: &Context,
    ) -> Result<Option<&'a mut SignedValidatorRegistration>, Error> {
        let state = self.state.read();
        let latest_timestamp = state
            .validator_preferences
            .get(&registration.message.public_key)
            .map(|r| r.message.timestamp);
        let message = &mut registration.message;

        validate_registration_is_not_from_future(message, current_timestamp)?;

        let registration_status = if let Some(latest_timestamp) = latest_timestamp {
            let status =
                determine_validator_registration_status(message.timestamp, latest_timestamp);
            if matches!(status, ValidatorRegistrationStatus::Outdated) {
                return Err(Error::OutdatedRegistration(message.clone(), latest_timestamp))
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
        validate_validator_status(message, validator_status)?;

        let signing_root = compute_builder_signing_root(message, context)?;
        let public_key = &message.public_key;
        verify_signature(public_key, signing_root.as_ref(), &registration.signature)?;

        let update = if matches!(registration_status, ValidatorRegistrationStatus::New) {
            trace!(%public_key, "processed new registration");
            Some(registration)
        } else {
            None
        };
        Ok(update)
    }

    pub fn process_registrations(
        &self,
        registrations: &mut [SignedValidatorRegistration],
        current_timestamp: u64,
        context: &Context,
    ) -> Result<(), Vec<Error>> {
        let (updates, errs): (Vec<_>, Vec<_>) = registrations
            .par_iter_mut()
            .map(|registration| self.process_registration(registration, current_timestamp, context))
            .partition(|result| result.is_ok());
        let mut state = self.state.write();
        for update in updates {
            if let Some(signed_registration) = update.expect("validated successfully") {
                let public_key = signed_registration.message.public_key.clone();
                state.validator_preferences.insert(public_key, signed_registration.clone());
            }
        }

        if errs.is_empty() {
            Ok(())
        } else {
            Err(errs.into_iter().map(|err| err.expect_err("validation failed")).collect())
        }
    }
}
