use crate::builder::{Builder, Error as BuilderError};
use crate::relay::{Error as RelayError, Relay};
use crate::types::{
    BidRequest, ExecutionPayload, Hash32, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration, Slot,
};
use async_trait::async_trait;
use beacon_api_client::Error as ApiError;
use futures::future::join_all;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::time;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no bids returned for proposal")]
    NoBidsReturned,
    #[error("could not find relay with outstanding bid to accept")]
    MissingOpenBid,
    #[error("could not register with any relay")]
    CouldNotRegister,
    #[error("{0}")]
    Relay(#[from] RelayError),
}

impl From<Error> for BuilderError {
    fn from(err: Error) -> Self {
        match err {
            Error::NoBidsReturned => Self::Custom(err.to_string()),
            Error::MissingOpenBid => Self::Custom(err.to_string()),
            Error::CouldNotRegister => Self::Custom(err.to_string()),
            Error::Relay(err) => match err {
                ApiError::Api(err) => Self::Api(err),
                err => Self::Internal(err.to_string()),
            },
        }
    }
}

async fn validate_bid(_bid: &SignedBuilderBid) -> Result<(), Error> {
    // TODO validations

    // verify builder signature

    // OPTIONAL:
    // verify payload header
    // -- parent_hash matches
    // -- fee recip matches, maybe
    // -- prev_randao matches
    // -- block_number matches
    // -- gas_limit is valid
    // -- timestamp is valid
    // -- base_fee_per_gas makes sense

    Ok(())
}

async fn validate_execution_payload(_execution_payload: &ExecutionPayload) -> Result<(), Error> {
    // TODO validations

    // optional ish
    // verify root matches root of corresponding header that was accepted

    Ok(())
}

#[derive(Clone)]
pub struct RelayMux(Arc<RelayMuxInner>);

impl std::ops::Deref for RelayMux {
    type Target = RelayMuxInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct RelayMuxInner {
    relays: Vec<Relay>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    // map from bid requests to index of `Relay` in collection
    outstanding_bids: HashMap<(Slot, Hash32), usize>,
}

impl RelayMux {
    pub fn new(relays: impl Iterator<Item = Relay>) -> Self {
        let inner = RelayMuxInner {
            relays: relays.collect(),
            state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    pub async fn run(&self) {
        // TODO purge expired state if a bid fails for some reason
        // - requires consensus clock...
        let mut interval = time::interval(Duration::from_secs(12));
        loop {
            interval.tick().await;
            let state = self.0.state.lock().unwrap();
            tracing::info!("{:?}", state);
        }
    }
}

#[async_trait]
impl Builder for RelayMux {
    async fn register_validator(
        &self,
        registration: &SignedValidatorRegistration,
    ) -> Result<(), BuilderError> {
        let responses = join_all(
            self.relays
                .iter()
                .map(|relay| async { relay.register_validator(registration).await }),
        )
        .await;

        let mut failures = vec![];
        let mut some_success = false;
        for response in responses {
            if let Err(err) = response {
                failures.push(err);
            } else {
                some_success = true;
            }
        }
        // TODO save failures to retry later
        if some_success {
            Ok(())
        } else {
            Err(Error::CouldNotRegister.into())
        }
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, BuilderError> {
        // TODO do not block on slow relays
        // TODO validate higher up the stack?
        let bids = join_all(
            self.relays
                .iter()
                .enumerate()
                .map(|(i, relay)| async move { (i, relay.fetch_best_bid(bid_request).await) }),
        )
        .await;

        // TODO allow for multiple relays to serve same bid
        let (relay_index, best_bid) = bids
            .into_iter()
            .filter_map(|(i, bid)| match bid {
                Ok(bid) => Some((i, bid)),
                Err(err) => {
                    tracing::warn!("{err}");
                    None
                }
            })
            .max_by_key(|(_, bid)| bid.message.value.clone())
            .ok_or(Error::NoBidsReturned)?;

        validate_bid(&best_bid).await?;

        let mut state = self.state.lock().unwrap();
        let key = (bid_request.slot, bid_request.parent_hash.clone());
        state.outstanding_bids.insert(key, relay_index);

        Ok(best_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BuilderError> {
        let relay_index = {
            let mut state = self.state.lock().unwrap();
            let key = bid_key_from(signed_block);
            match state.outstanding_bids.remove(&key) {
                Some(relay_index) => relay_index,
                None => return Err(Error::MissingOpenBid.into()),
            }
        };

        let relay = &self.relays[relay_index];
        let payload = relay.open_bid(signed_block).await?;

        validate_execution_payload(&payload).await?;

        Ok(payload)
    }
}

fn bid_key_from(signed_block: &SignedBlindedBeaconBlock) -> (Slot, Hash32) {
    let block = &signed_block.message;

    (
        block.slot,
        block.body.execution_payload_header.parent_hash.clone(),
    )
}
