use crate::validator_summary_provider::{
    Error as ValidatorSummaryProviderError, ValidatorSummaryProvider,
};
use async_trait::async_trait;
use beacon_api_client::{Client, ValidatorStatus};
use ethereum_consensus::{
    builder::ValidatorRegistration,
    clock,
    clock::get_current_unix_time_in_secs,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Slot, U256},
    state_transition::{Context, Error as ConsensusError},
};
use futures::StreamExt;
use mev_build_rs::{
    sign_builder_message, verify_signed_builder_message, verify_signed_consensus_message,
    BidRequest, BlindedBlockProvider, BlindedBlockProviderError,  BuilderError,
    EngineBuilder, ExecutionPayload,
    SignedBlindedBeaconBlock, SignedBuilderBid, SignedValidatorRegistration,
    types::bellatrix as bellatrix
};
use parking_lot::Mutex;
use std::{cmp::Ordering, collections::HashMap, ops::Deref, sync::Arc};
use thiserror::Error;

// `PROPOSAL_TOLERANCE_DELAY` controls how aggresively the relay drops "old" execution payloads
// once they have been fetched from builders -- currently in response to an incoming request from a
// proposer. Setting this to anything other than `0` incentivizes late proposals and setting it to
// `1` allows for latency at the slot boundary while still enabling a successful proposal.
// TODO likely drop this feature...
const PROPOSAL_TOLERANCE_DELAY: Slot = 1;

#[derive(Debug, Error)]
pub enum Error {
    #[error("unknown parent hash in proposal request")]
    UnknownHash,
    #[error("unknown validator with pubkey in proposal request")]
    UnknownValidator,
    #[error("unknown fee recipient for proposer given in proposal request")]
    UnknownFeeRecipient,
    #[error("block does not match the provided header")]
    UnknownBlock,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid timestamp")]
    InvalidTimestamp,
    #[error("payload request does not match any outstanding bid")]
    UnknownBid,
    #[error("payload gas limit does not match the proposer's preference")]
    InvalidGasLimit,
    #[error("validator {0} had an invalid status {1}")]
    InactiveValidator(BlsPublicKey, ValidatorStatus),
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error("{0}")]
    Builder(#[from] BuilderError),
    #[error("{0}")]
    Validators(#[from] ValidatorSummaryProviderError),
}

impl From<Error> for BlindedBlockProviderError {
    fn from(err: Error) -> Self {
        match err {
            Error::Consensus(err) => err.into(),
            // TODO conform to API errors
            err => Self::Custom(err.to_string()),
        }
    }
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
    if matches!(status, ValidatorStatus::Active | ValidatorStatus::Pending) {
        Ok(())
    } else {
        Err(Error::InactiveValidator(public_key.clone(), status))
    }
}

fn validate_registration(
    validators: &ValidatorSummaryProvider,
    registration: &mut SignedValidatorRegistration,
    current_timestamp: u64,
    latest_timestamp: Option<u64>,
    context: &Context,
) -> Result<ValidatorRegistrationStatus, Error> {
    let message = &mut registration.message;

    validate_registration_is_not_from_future(message.timestamp, current_timestamp)?;

    let registration_status = if let Some(latest_timestamp) = latest_timestamp {
        let status = determine_validator_registration_status(message.timestamp, latest_timestamp);
        if matches!(status, ValidatorRegistrationStatus::Outdated) {
            return Err(Error::InvalidTimestamp)
        }
        status
    } else {
        ValidatorRegistrationStatus::New
    };

    let validator_status = validators.get_status(&message.public_key)?;
    validate_validator_status(validator_status, &message.public_key)?;

    let public_key = message.public_key.clone();
    verify_signed_builder_message(message, &registration.signature, &public_key, context)?;

    Ok(registration_status)
}

fn validate_bid_request(_bid_request: &BidRequest) -> Result<(), Error> {
    // TODO validations

    // verify slot is timely

    // verify parent_hash is on a chain tip

    // verify public_key is one of the possible proposers

    Ok(())
}

fn validate_execution_payload(
    execution_payload: &ExecutionPayload,
    _value: &U256,
    preferences: &ValidatorRegistration,
) -> Result<(), Error> {
    // TODO validations

    // TODO allow for "adjustment cap" per the protocol rules
    // towards the proposer's preference
    if execution_payload.gas_limit != preferences.gas_limit {
        return Err(Error::InvalidGasLimit)
    }

    // verify payload is valid

    // verify payload sends `value` to proposer

    Ok(())
}

fn validate_signed_block(
    signed_block: &mut SignedBlindedBeaconBlock,
    public_key: &BlsPublicKey,
    payload: &mut ExecutionPayload,
    context: &Context,
) -> Result<(), Error> {
    let header = ExecutionPayloadHeader::try_from(payload)?;
    if signed_block.message.body.execution_payload_header != header {
        return Err(Error::UnknownBlock)
    }

    let message = &mut signed_block.message;
    verify_signed_consensus_message(message, &signed_block.signature, public_key, context)?;

    // OPTIONAL:
    // -- verify w/ consensus?
    // verify slot is timely
    // verify proposer_index is correct
    // verify parent_root matches
    Ok(())
}

#[derive(Clone)]
pub struct Relay(Arc<RelayInner>);

impl Deref for Relay {
    type Target = RelayInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct RelayInner {
    secret_key: SecretKey,
    public_key: BlsPublicKey,
    builder: EngineBuilder,
    validators: ValidatorSummaryProvider,
    context: Arc<Context>,
    state: Mutex<State>,
}

impl RelayInner {
    pub fn new(
        secret_key: SecretKey,
        builder: EngineBuilder,
        validators: ValidatorSummaryProvider,
        context: Arc<Context>,
    ) -> Self {
        let public_key = secret_key.public_key();
        Self { secret_key, public_key, context, builder, validators, state: Default::default() }
    }
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
    execution_payloads: HashMap<BidRequest, ExecutionPayload>,
}

impl Relay {
    pub fn new(builder: EngineBuilder, beacon_node: Client, context: Arc<Context>) -> Self {
        let key_bytes = [1u8; 32];
        let secret_key = SecretKey::try_from(key_bytes.as_slice()).unwrap();
        let validators = ValidatorSummaryProvider::new(beacon_node);
        let inner = RelayInner::new(secret_key, builder, validators, context);
        Self(Arc::new(inner))
    }

    async fn load_full_validator_set(&self) {
        if let Err(err) = self.validators.load().await {
            tracing::error!("could not load validator set from provided beacon node; please check config and restart: {err}");
        }
    }

    pub async fn initialize(&self) {
        self.load_full_validator_set().await;
    }

    pub async fn run(&self) {
        let clock = clock::for_mainnet();
        let slots = clock.stream_slots();

        tokio::pin!(slots);

        let mut current_epoch = clock.current_epoch();
        while let Some(slot) = slots.next().await {
            let epoch = clock.epoch_for(slot);
            if epoch > current_epoch {
                current_epoch = epoch;
                // TODO grab validators more efficiently
                self.load_full_validator_set().await;
            }
            let mut state = self.state.lock();
            state
                .execution_payloads
                .retain(|bid_request, _| bid_request.slot + PROPOSAL_TOLERANCE_DELAY >= slot);
        }
    }
}

#[async_trait]
impl BlindedBlockProvider for Relay {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        let mut new_registrations = {
            let mut state = self.state.lock();
            let current_time = get_current_unix_time_in_secs();
            let mut new_registrations = vec![];
            for registration in registrations.iter_mut() {
                let latest_timestamp = state
                    .validator_preferences
                    .get(&registration.message.public_key)
                    .map(|registration| registration.message.timestamp);

                // TODO one failure should not fail the others...
                let status = validate_registration(
                    &self.validators,
                    registration,
                    current_time,
                    latest_timestamp,
                    &self.context,
                )?;

                if matches!(status, ValidatorRegistrationStatus::New) {
                    let public_key = registration.message.public_key.clone();
                    state.validator_preferences.insert(public_key.clone(), registration.clone());
                    new_registrations.push(registration.clone());
                }
            }
            new_registrations
        };
        self.builder.register_validators(&mut new_registrations)?;
        Ok(())
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, BlindedBlockProviderError> {
        validate_bid_request(bid_request)?;

        let (mut payload, value) = self.builder.get_payload_with_value(bid_request)?;

        let header = {
            let mut state = self.state.lock();

            let preferences = state
                .validator_preferences
                .get(&bid_request.public_key)
                .ok_or(Error::UnknownValidator)?;

            validate_execution_payload(&payload, &value, &preferences.message)?;

            let header = ExecutionPayloadHeader::try_from(&mut payload)?;

            state.execution_payloads.insert(bid_request.clone(), payload);
            header
        };

        let mut bid = BuilderBid { header, value, public_key: self.public_key.clone() };

        let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;

        let signed_bid = SignedBuilderBid::Bellatrix() { message: bid, signature };
        Ok(signed_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BlindedBlockProviderError> {
        let block = &signed_block.message;
        let public_key =
            self.validators.get_public_key(block.proposer_index).map_err(Error::from)?;
        let bid_request = BidRequest {
            slot: block.slot,
            parent_hash: block.body.execution_payload_header.parent_hash.clone(),
            public_key,
        };

        let mut payload = {
            let mut state = self.state.lock();
            state.execution_payloads.remove(&bid_request).ok_or(Error::UnknownBid)?
        };

        validate_signed_block(signed_block, &bid_request.public_key, &mut payload, &self.context)?;

        Ok(payload)
    }
}
