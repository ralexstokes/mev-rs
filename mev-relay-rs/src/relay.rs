use async_trait::async_trait;
use beacon_api_client::ApiError;
use ethereum_consensus::builder::{compute_builder_domain, ValidatorRegistration};
use ethereum_consensus::crypto::SecretKey;
use ethereum_consensus::phase0::mainnet::Context;
use ethereum_consensus::phase0::verify_signed_data;
use ethereum_consensus::primitives::{BlsPublicKey, ExecutionAddress};
use http::StatusCode;
use mev_build_rs::{
    BidRequest, Builder, BuilderBid, Error as BuilderError, ExecutionPayload,
    ExecutionPayloadHeader, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use ssz_rs::prelude::{Merkleized, U256};
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
    #[error("{0}")]
    Custom(String),
}

impl From<Error> for BuilderError {
    fn from(err: Error) -> Self {
        match err {
            Error::Custom(msg) => Self::Custom(msg),
            err => Self::Api(ApiError {
                code: StatusCode::BAD_REQUEST,
                message: err.to_string(),
            }),
        }
    }
}

async fn validate_registration(
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

    // TODO: avoid clones here (?)
    let public_key = registration.message.public_key.clone();
    // TODO: error handling
    let domain = compute_builder_domain(context).map_err(|err| Error::Custom(err.to_string()))?;
    verify_signed_data(
        &mut registration.message,
        &registration.signature,
        &public_key,
        domain,
    )
    .map_err(|err| Error::Custom(err.to_string()))
}

async fn validate_bid_request(_bid_request: &BidRequest) -> Result<(), Error> {
    // TODO validations

    // verify slot is timely

    // verify parent_hash is on a chain tip

    // verify public_key is one of the possible proposers

    Ok(())
}

async fn validate_signed_block(_signed_block: &mut SignedBlindedBeaconBlock) -> Result<(), Error> {
    // TODO validations

    // verify signature
    // NOTE: need access to validator set

    // OPTIONAL:
    // verify slot is timely
    // verify proposer_index is correct
    // verify parent_root matches
    // verify payload header matches the one we sent out
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Relay {
    state: Arc<Mutex<State>>,
    secret_key: SecretKey,
    builder_key: BlsPublicKey,
    context: Arc<Context>,
}

impl Relay {
    pub fn new(context: Arc<Context>) -> Self {
        let key_bytes = [1u8; 32];
        let secret_key = SecretKey::from_bytes(&key_bytes).unwrap();
        let builder_key = secret_key.public_key();
        Self {
            state: Default::default(),
            secret_key,
            builder_key,
            context,
        }
    }
}

#[derive(Debug)]
struct ValidatorPreferences {
    pub fee_recipient: ExecutionAddress,
    pub _gas_limit: u64,
    pub timestamp: u64,
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, ValidatorPreferences>,
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

#[async_trait]
impl Builder for Relay {
    async fn register_validator(
        &self,
        registration: &mut SignedValidatorRegistration,
    ) -> Result<(), BuilderError> {
        let public_key = registration.message.public_key.clone();

        let latest_timestamp = {
            let state = self.state.lock().expect("can lock");
            state
                .validator_preferences
                .get(&public_key)
                .map(|preferences| preferences.timestamp)
        };

        validate_registration(registration, latest_timestamp, &self.context).await?;

        let preferences = ValidatorPreferences::from(&registration.message);

        let mut state = self.state.lock().expect("can lock");
        state.validator_preferences.insert(public_key, preferences);
        Ok(())
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &mut BidRequest,
    ) -> Result<SignedBuilderBid, BuilderError> {
        validate_bid_request(bid_request).await?;

        let public_key = &bid_request.public_key;

        let state = self.state.lock().unwrap();
        let fee_recipient = state
            .validator_preferences
            .get(public_key)
            .map(|p| &p.fee_recipient)
            .ok_or(Error::UnknownValidator)?;

        let mut bid = BuilderBid {
            header: ExecutionPayloadHeader {
                parent_hash: bid_request.parent_hash.clone(),
                fee_recipient: fee_recipient.clone(),
                ..Default::default()
            },
            value: U256::from_bytes_le([1u8; 32]),
            public_key: self.builder_key.clone(),
        };

        let signature = self.secret_key.sign(bid.hash_tree_root().unwrap().as_ref());

        let signed_bid = SignedBuilderBid {
            message: bid,
            signature,
        };

        // TODO validate?

        Ok(signed_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BuilderError> {
        validate_signed_block(signed_block).await?;

        let block = &signed_block.message;
        let header = &block.body.execution_payload_header;

        let payload = ExecutionPayload {
            parent_hash: header.parent_hash.clone(),
            fee_recipient: header.fee_recipient.clone(),
            ..Default::default()
        };
        Ok(payload)
    }
}
