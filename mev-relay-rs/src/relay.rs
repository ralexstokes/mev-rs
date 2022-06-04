use async_trait::async_trait;
use ethereum_consensus::builder::ValidatorRegistration;
use ethereum_consensus::clock::get_current_unix_time_in_secs;
use ethereum_consensus::crypto::SecretKey;
use ethereum_consensus::primitives::{BlsPublicKey, U256};
use ethereum_consensus::state_transition::{Context, Error as ConsensusError};
use mev_build_rs::{
    sign_builder_message, verify_signed_builder_message, verify_signed_consensus_message,
    BidRequest, BlindedBlockProvider, BlindedBlockProviderError, BuilderBid, BuilderError,
    EngineBuilder, ExecutionPayload, ExecutionPayloadHeader, ExecutionPayloadWithValue,
    SignedBlindedBeaconBlock, SignedBuilderBid, SignedValidatorRegistration,
};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use thiserror::Error;

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
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error("{0}")]
    Builder(#[from] BuilderError),
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

fn validate_registration(
    registration: &mut SignedValidatorRegistration,
    current_timestamp: u64,
    latest_timestamp: Option<u64>,
    context: &Context,
) -> Result<ValidatorRegistrationStatus, Error> {
    let message = &mut registration.message;

    validate_registration_is_not_from_future(message.timestamp, current_timestamp)?;

    let status = if let Some(latest_timestamp) = latest_timestamp {
        let status = determine_validator_registration_status(message.timestamp, latest_timestamp);
        if matches!(status, ValidatorRegistrationStatus::Outdated) {
            return Err(Error::InvalidTimestamp);
        }
        status
    } else {
        ValidatorRegistrationStatus::New
    };

    // TODO check once we have pubkey index
    // pubkey is active or in entry queue
    // -- `is_eligible_for_activation` || `is_active_validator`

    let public_key = message.public_key.clone();
    verify_signed_builder_message(message, &registration.signature, &public_key, context)?;

    Ok(status)
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
        return Err(Error::InvalidGasLimit);
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
    // TODO validations

    // verify payload header matches the one we sent out
    // TODO can skip allocations if `impl PartialEq<Header> for Payload`
    let header = ExecutionPayloadHeader::try_from(payload)?;
    if signed_block.message.body.execution_payload_header != header {
        return Err(Error::UnknownBlock);
    }

    // verify signature
    let message = &mut signed_block.message;
    // TODO restore `?` once public keys are accurate
    let _ = verify_signed_consensus_message(message, &signed_block.signature, public_key, context);

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

impl Relay {
    pub fn new(context: Context) -> Self {
        let key_bytes = [1u8; 32];
        let secret_key = SecretKey::try_from(key_bytes.as_slice()).unwrap();
        let inner = RelayInner::new(secret_key, context);
        Self(Arc::new(inner))
    }
}

pub struct RelayInner {
    secret_key: SecretKey,
    public_key: BlsPublicKey,
    builder: EngineBuilder,
    context: Context,
    state: Mutex<State>,
}

impl RelayInner {
    pub fn new(secret_key: SecretKey, context: Context) -> Self {
        let public_key = secret_key.public_key();
        let builder = EngineBuilder::new(context.clone());
        Self {
            secret_key,
            public_key,
            context,
            builder,
            state: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
    execution_payloads: HashMap<BidRequest, ExecutionPayload>,
}

#[async_trait]
impl BlindedBlockProvider for Relay {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        let mut new_registrations = {
            let mut state = self.state.lock().expect("can lock");
            let current_time = get_current_unix_time_in_secs();
            let mut new_registrations = vec![];
            for registration in registrations.iter_mut() {
                let latest_timestamp = state
                    .validator_preferences
                    .get(&registration.message.public_key)
                    .map(|registration| registration.message.timestamp);

                // TODO one failure should not fail the others...
                let status = validate_registration(
                    registration,
                    current_time,
                    latest_timestamp,
                    &self.context,
                )?;

                if matches!(status, ValidatorRegistrationStatus::New) {
                    let public_key = registration.message.public_key.clone();
                    state
                        .validator_preferences
                        .insert(public_key.clone(), registration.clone());
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

        let ExecutionPayloadWithValue { mut payload, value } =
            self.builder.get_payload_with_value(bid_request)?;

        let mut state = self.state.lock().expect("can lock");

        let preferences = state
            .validator_preferences
            .get(&bid_request.public_key)
            .ok_or(Error::UnknownValidator)?;

        validate_execution_payload(&payload, &value, &preferences.message)?;

        let header = ExecutionPayloadHeader::try_from(&mut payload)?;

        // TODO restore public key once we can look them up correctly
        // let bid_request = bid_request.clone();
        let bid_request = BidRequest {
            slot: bid_request.slot,
            parent_hash: bid_request.parent_hash.clone(),
            ..Default::default()
        };
        state.execution_payloads.insert(bid_request, payload);

        let mut bid = BuilderBid {
            header,
            value,
            public_key: self.public_key.clone(),
        };

        let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;

        let signed_bid = SignedBuilderBid {
            message: bid,
            signature,
        };
        Ok(signed_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BlindedBlockProviderError> {
        let block = &signed_block.message;
        // TODO get correct public key for proposer
        // NOTE: need access to validator set
        let proposer_public_key = BlsPublicKey::default();
        let bid_request = BidRequest {
            slot: block.slot,
            parent_hash: block.body.execution_payload_header.parent_hash.clone(),
            public_key: proposer_public_key,
        };

        let mut state = self.state.lock().expect("can lock");
        let mut payload = state
            .execution_payloads
            .remove(&bid_request)
            .ok_or(Error::UnknownBid)?;

        validate_signed_block(
            signed_block,
            &bid_request.public_key,
            &mut payload,
            &self.context,
        )?;

        Ok(payload)
    }
}
