use async_trait::async_trait;
use ethereum_consensus::clock;
use ethereum_consensus::primitives::Hash32;
use ethereum_consensus::state_transition::{Context, Error as ConsensusError};
use futures::StreamExt;
use mev_build_rs::{
    verify_signed_builder_message, ApiClient as Relay, BidRequest, Builder, Error as BuilderError,
    ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid, SignedValidatorRegistration,
};
use ssz_rs::prelude::U256;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

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
    Consensus(#[from] ConsensusError),
}

impl From<Error> for BuilderError {
    fn from(err: Error) -> Self {
        match err {
            Error::Consensus(err) => err.into(),
            // TODO conform to API errors
            err => Self::Custom(err.to_string()),
        }
    }
}

async fn validate_bid(bid: &mut SignedBuilderBid, context: &Context) -> Result<(), Error> {
    let message = &mut bid.message;
    let public_key = message.public_key.clone();
    verify_signed_builder_message(message, &bid.signature, &public_key, context)?;
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
    context: Arc<Context>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    // map from bid requests to index of `Relay` in collection
    outstanding_bids: HashMap<BidRequest, Vec<usize>>,
}

impl RelayMux {
    pub fn new(relays: impl Iterator<Item = Relay>, context: Arc<Context>) -> Self {
        let inner = RelayMuxInner {
            relays: relays.collect(),
            context,
            state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    pub async fn run(&self) {
        let clock = clock::for_mainnet();
        let slots = clock.stream_slots();

        tokio::pin!(slots);

        while let Some(slot) = slots.next().await {
            let state = self.0.state.lock().unwrap();
            tracing::debug!(
                "slot {slot}: outstanding bids: {:?}",
                state.outstanding_bids
            );
        }
    }
}

#[async_trait]
impl Builder for RelayMux {
    async fn register_validator(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BuilderError> {
        // TODO (and below) do this concurrently?
        let mut responses = vec![];
        for relay in &self.relays {
            responses.push(relay.register_validator(registrations).await)
        }

        let failures = responses.iter().filter(|r| r.is_err());

        if failures.count() == self.relays.len() {
            Err(Error::CouldNotRegister.into())
        } else {
            Ok(())
        }
    }

    async fn fetch_best_bid(
        &self,
        bid_request: &mut BidRequest,
    ) -> Result<SignedBuilderBid, BuilderError> {
        let mut bids = vec![];
        for relay in &self.relays {
            bids.push(relay.fetch_best_bid(bid_request).await)
        }

        let mut best_bid_value = U256::zero();
        let mut best_bids = vec![];
        for (i, bid) in bids.into_iter().enumerate() {
            match bid {
                Ok(mut bid) => {
                    if let Err(err) = validate_bid(&mut bid, &self.context).await {
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
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BuilderError> {
        let relay_indices = {
            let mut state = self.state.lock().unwrap();
            let key = bid_key_from(signed_block);
            match state.outstanding_bids.remove(&key) {
                Some(indices) => indices,
                None => return Err(Error::MissingOpenBid.into()),
            }
        };

        let mut responses = vec![];
        for i in relay_indices {
            let relay = &self.relays[i];
            responses.push(relay.open_bid(signed_block).await);
        }

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
