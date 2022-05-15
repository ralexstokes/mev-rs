use async_trait::async_trait;
use beacon_api_client::ApiError;
use ethereum_consensus::primitives::{BlsPublicKey, ExecutionAddress};
use http::StatusCode;
use mev_build_rs::{
    BidRequest, Builder, BuilderBid, Error as BuilderError, ExecutionPayload,
    ExecutionPayloadHeader, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use ssz_rs::prelude::U256;
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

async fn validate_registration(_registration: &SignedValidatorRegistration) -> Result<(), Error> {
    // TODO validations

    // track timestamps
    // -- must be greater than previous successful announcement
    // -- if more than 10 seconds in future, error

    // pubkey is active or in entry queue
    // -- `is_eligible_for_activation` || `is_active_validator`

    // verify signature
    Ok(())
}

async fn validate_bid_request(_bid_request: &BidRequest) -> Result<(), Error> {
    // TODO validations

    // verify slot is timely

    // verify parent_hash is on a chain tip

    // verify public_key is one of the possible proposers

    Ok(())
}

async fn validate_signed_block(_signed_block: &SignedBlindedBeaconBlock) -> Result<(), Error> {
    // TODO validations

    // verify signature

    // OPTIONAL:
    // verify slot is timely
    // verify proposer_index is correct
    // verify parent_root matches
    // verify payload header matches the one we sent out
    Ok(())
}

#[derive(Debug, Default, Clone)]
pub struct Relay {
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    fee_recipients: HashMap<BlsPublicKey, ExecutionAddress>,
}

#[async_trait]
impl Builder for Relay {
    async fn register_validator(
        &self,
        registration: &SignedValidatorRegistration,
    ) -> Result<(), BuilderError> {
        validate_registration(registration).await?;

        let mut state = self.state.lock().expect("can lock");
        let registration = &registration.message;
        state.fee_recipients.insert(
            registration.public_key.clone(),
            registration.fee_recipient.clone(),
        );
        Ok(())
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, BuilderError> {
        validate_bid_request(bid_request).await?;

        let public_key = &bid_request.public_key;

        let state = self.state.lock().unwrap();
        let fee_recipient = state
            .fee_recipients
            .get(public_key)
            .ok_or(Error::UnknownValidator)?;

        let bid = BuilderBid {
            header: ExecutionPayloadHeader {
                parent_hash: bid_request.parent_hash.clone(),
                fee_recipient: fee_recipient.clone(),
                ..Default::default()
            },
            value: U256::from_bytes_le([1u8; 32]),
            public_key: Default::default(),
        };

        let signed_bid = SignedBuilderBid {
            message: bid,
            ..Default::default()
        };

        // TODO validate?

        Ok(signed_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
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
