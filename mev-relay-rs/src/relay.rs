use async_trait::async_trait;
use ethereum_consensus::builder::ValidatorRegistration;
use ethereum_consensus::crypto::SecretKey;
use ethereum_consensus::primitives::{BlsPublicKey, ExecutionAddress, U256};
use ethereum_consensus::state_transition::{Context, Error as ConsensusError};
use mev_build_rs::{
    sign_builder_message, verify_signed_builder_message, verify_signed_consensus_message,
    BidRequest, BlindedBlockProvider, BlindedBlockProviderError, BuilderBid, BuilderError,
    EngineBuilder, ExecutionPayload, ExecutionPayloadHeader, ExecutionPayloadWithValue,
    SignedBlindedBeaconBlock, SignedBuilderBid, SignedValidatorRegistration,
};
use std::collections::HashMap;
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

fn validate_registration(
    registration: &mut SignedValidatorRegistration,
    _latest_timestamp: Option<u64>,
    context: &Context,
) -> Result<(), Error> {
    // TODO validations

    // track timestamps
    // -- must be greater than previous successful announcement
    // -- if more than 10 seconds in future, error

    // pubkey is active or in entry queue
    // -- `is_eligible_for_activation` || `is_active_validator`

    let message = &mut registration.message;
    let public_key = message.public_key.clone();
    verify_signed_builder_message(message, &registration.signature, &public_key, context)?;
    Ok(())
}

fn validate_bid_request(_bid_request: &BidRequest) -> Result<(), Error> {
    // TODO validations

    // verify slot is timely

    // verify parent_hash is on a chain tip

    // verify public_key is one of the possible proposers

    Ok(())
}

fn validate_execution_payload(
    _execution_payload: &ExecutionPayload,
    _value: &U256,
    _preferences: &ValidatorPreferences,
) -> Result<(), Error> {
    // TODO validations

    // verify gas limit respects validator preferences

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

#[derive(Debug)]
struct ValidatorPreferences {
    pub fee_recipient: ExecutionAddress,
    pub _gas_limit: u64,
    pub timestamp: u64,
}

impl From<&ValidatorRegistration> for ValidatorPreferences {
    fn from(registration: &ValidatorRegistration) -> Self {
        Self {
            fee_recipient: registration.fee_recipient.clone(),
            _gas_limit: registration.gas_limit,
            timestamp: registration.timestamp,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Relay {
    secret_key: SecretKey,
    public_key: BlsPublicKey,
    inner: Arc<RelayInner>,
}

impl Relay {
    pub fn new(context: Context) -> Self {
        let key_bytes = [1u8; 32];
        let secret_key = SecretKey::try_from(key_bytes.as_slice()).unwrap();
        let public_key = secret_key.public_key();
        let inner = RelayInner::new(context);
        Self {
            secret_key,
            public_key,
            inner: Arc::new(inner),
        }
    }
}

#[derive(Debug, Default)]
struct RelayInner {
    state: Mutex<State>,
    builder: EngineBuilder,
    context: Context,
}

impl RelayInner {
    pub fn new(context: Context) -> Self {
        Self {
            context,
            ..Default::default()
        }
    }
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, ValidatorPreferences>,
    execution_payloads: HashMap<BidRequest, ExecutionPayload>,
}

#[async_trait]
impl BlindedBlockProvider for Relay {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        // TODO parallelize?
        for registration in registrations.iter_mut() {
            let latest_timestamp = {
                let state = self.inner.state.lock().expect("can lock");
                state
                    .validator_preferences
                    .get(&registration.message.public_key)
                    .map(|preferences| preferences.timestamp)
            };

            validate_registration(registration, latest_timestamp, &self.inner.context)?;

            let preferences = ValidatorPreferences::from(&registration.message);
            let public_key = registration.message.public_key.clone();

            let mut state = self.inner.state.lock().expect("can lock");
            state.validator_preferences.insert(public_key, preferences);
        }
        Ok(())
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, BlindedBlockProviderError> {
        validate_bid_request(bid_request)?;

        let ExecutionPayloadWithValue { mut payload, value } =
            self.inner.builder.get_payload_with_value(bid_request)?;

        let mut state = self.inner.state.lock().expect("can lock");

        let preferences = state
            .validator_preferences
            .get(&bid_request.public_key)
            .ok_or(Error::UnknownValidator)?;

        // TODO remove once this logic moves into the builder
        payload.fee_recipient = preferences.fee_recipient.clone();
        validate_execution_payload(&payload, &value, preferences)?;

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

        let signature = sign_builder_message(&mut bid, &self.secret_key, &self.inner.context)?;

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

        let mut state = self.inner.state.lock().expect("can lock");
        let mut payload = state
            .execution_payloads
            .remove(&bid_request)
            .ok_or(Error::UnknownBid)?;

        validate_signed_block(
            signed_block,
            &bid_request.public_key,
            &mut payload,
            &self.inner.context,
        )?;

        Ok(payload)
    }
}
