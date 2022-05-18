use async_trait::async_trait;
use beacon_api_client::Error as ApiError;
use ethereum_consensus::primitives::Hash32;
use futures::future::join_all;
use mev_build_rs::{
    BidRequest, Builder, Error as BuilderError, ExecutionPayload, SignedBlindedBeaconBlock,
    SignedBuilderBid, SignedValidatorRegistration,
};
use mev_relay_rs::{Client as Relay, ClientError as RelayError};
use ssz_rs::prelude::MerkleizationError;
use ssz_rs::prelude::U256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::time;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no valid bids returned for proposal")]
    NoBidsReturned,
    #[error("could not find relay with outstanding bid to accept")]
    MissingOpenBid,
    #[error("could not register with any relay")]
    CouldNotRegister,
    #[error("no payload returned for opened bid with block hash {0}")]
    MissingPayload(Hash32),
    #[error("{0}")]
    Relay(#[from] RelayError),
    #[error("{0}")]
    Merkleization(#[from] MerkleizationError),
    #[error("invalid signature")]
    InvalidSignature,
}

impl From<Error> for BuilderError {
    fn from(err: Error) -> Self {
        match err {
            Error::Relay(err) => match err {
                ApiError::Api(err) => Self::Api(err),
                err => Self::Internal(err.to_string()),
            },
            err => Self::Custom(err.to_string()),
        }
    }
}

async fn validate_bid(_bid: &mut SignedBuilderBid) -> Result<(), Error> {
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
    outstanding_bids: HashMap<BidRequest, Vec<usize>>,
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
        let mut interval = time::interval(Duration::from_secs(12));
        loop {
            interval.tick().await;
            let state = self.0.state.lock().unwrap();
            tracing::debug!("outstanding bids: {:?}", state.outstanding_bids);
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

        let failures = responses.iter().filter(|r| r.is_err());

        if failures.count() == self.relays.len() {
            Err(Error::CouldNotRegister.into())
        } else {
            Ok(())
        }
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, BuilderError> {
        let bids = join_all(
            self.relays
                .iter()
                .map(|relay| async move { relay.fetch_best_bid(bid_request).await }),
        )
        .await;

        let mut best_bid_value = U256::default();
        let mut best_bids = vec![];
        for (i, bid) in bids.into_iter().enumerate() {
            match bid {
                Ok(mut bid) => {
                    if let Err(err) = validate_bid(&mut bid).await {
                        tracing::warn!("invalid signed builder bid: {err} for bid: {bid:?}");
                        continue;
                    }

                    let value = &bid.message.value;
                    if value > &best_bid_value {
                        best_bid_value = value.clone();
                        best_bids.clear();
                    }
                    if value == &best_bid_value {
                        best_bids.push((i, bid));
                    }
                }
                Err(err) => {
                    tracing::warn!("issue with relay {i}: {err}");
                }
            }
        }

        if best_bids.is_empty() {
            return Err(Error::NoBidsReturned.into());
        }

        let ((i, best_bid), rest) = best_bids.split_first().unwrap();
        let mut relay_indices = vec![*i];
        for (i, bid) in rest {
            if bid.message.header.block_hash == best_bid.message.header.block_hash {
                relay_indices.push(*i);
            }
        }

        let mut state = self.state.lock().unwrap();
        let key = BidRequest {
            public_key: Default::default(),
            ..bid_request.clone()
        };
        state.outstanding_bids.insert(key, relay_indices);

        Ok(best_bid.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BuilderError> {
        let relay_indices = {
            let mut state = self.state.lock().unwrap();
            let key = bid_key_from(signed_block);
            match state.outstanding_bids.remove(&key) {
                Some(indices) => indices,
                None => return Err(Error::MissingOpenBid.into()),
            }
        };

        let responses = join_all(relay_indices.into_iter().map(|i| async move {
            let relay = &self.relays[i];
            relay.open_bid(signed_block).await
        }))
        .await;

        let mut opened_payload = None;
        let expected_block_hash = &signed_block
            .message
            .body
            .execution_payload_header
            .block_hash;
        for (i, response) in responses.into_iter().enumerate() {
            match response {
                Ok(payload) => {
                    if &payload.block_hash == expected_block_hash {
                        opened_payload = Some(payload);
                    } else {
                        tracing::warn!("error opening bid: the returned payload from relay {i} did not match the expected block hash: {expected_block_hash:?}");
                    }
                }
                Err(err) => {
                    tracing::warn!("error opening bid: {err}");
                }
            }
        }

        opened_payload.ok_or_else(|| Error::MissingPayload(expected_block_hash.clone()).into())
    }
}

fn bid_key_from(signed_block: &SignedBlindedBeaconBlock) -> BidRequest {
    let block = &signed_block.message;

    BidRequest {
        slot: block.slot,
        parent_hash: block.body.execution_payload_header.parent_hash.clone(),
        // TODO get public key
        public_key: Default::default(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_bid_selection() {
        // ensure max value is kept
        // ensure values are all the same
        // if diff payloads, then pick by block hash
    }
}
