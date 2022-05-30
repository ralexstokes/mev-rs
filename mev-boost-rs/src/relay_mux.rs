use async_trait::async_trait;
use ethereum_consensus::clock;
use ethereum_consensus::primitives::{Hash32, U256};
use ethereum_consensus::state_transition::{Context, Error as ConsensusError};
use futures::{stream, StreamExt};
use mev_build_rs::{
    verify_signed_builder_message, BidRequest, BlindedBlockProvider,
    BlindedBlockProviderClient as Relay, BlindedBlockProviderError, ExecutionPayload,
    SignedBlindedBeaconBlock, SignedBuilderBid, SignedValidatorRegistration,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no valid bids returned for proposal")]
    NoBids,
    #[error("could not find relay with outstanding bid to accept")]
    MissingOpenBid,
    #[error("could not register with any relay")]
    CouldNotRegister,
    #[error("no payload returned for opened bid with block hash {0}")]
    MissingPayload(Hash32),
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
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

fn validate_bid(bid: &mut SignedBuilderBid, context: &Context) -> Result<(), Error> {
    let message = &mut bid.message;
    let public_key = message.public_key.clone();
    verify_signed_builder_message(message, &bid.signature, &public_key, context)?;
    Ok(())
}

// Select the most valuable bids in `bids`, breaking ties by `block_hash`
fn select_best_bids<'a>(bids: impl Iterator<Item = (&'a U256, usize)>) -> Vec<usize> {
    let mut best_value = U256::zero();
    bids.fold(vec![], |mut relay_indices, (value, index)| {
        if value > &best_value {
            best_value = value.clone();
            relay_indices.clear();
        }
        if value == &best_value {
            relay_indices.push(index);
        }
        relay_indices
    })
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
    // TODO: this can likely be faster by just having a list of URLs
    // and one shared API client
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
impl BlindedBlockProvider for RelayMux {
    async fn register_validator(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        let registrations = &registrations;
        let responses = stream::iter(self.relays.iter().cloned())
            .map(|relay| async move { relay.register_validator(registrations).await })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
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
    ) -> Result<SignedBuilderBid, BlindedBlockProviderError> {
        let responses = stream::iter(self.relays.iter().cloned())
            .map(|relay| async move { relay.fetch_best_bid(bid_request).await })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
            .await;

        // ideally can fuse the filtering into the prior async fetch but
        // several attempts lead to opaque compiler errors...
        let bids = responses
            .into_iter()
            .enumerate()
            .filter_map(|(relay_index, response)| match response {
                Ok(mut bid) => {
                    if let Err(err) = validate_bid(&mut bid, &self.context) {
                        tracing::warn!("invalid signed builder bid: {err} for bid: {bid:?}");
                        None
                    } else {
                        Some((bid, relay_index))
                    }
                }
                Err(err) => {
                    tracing::warn!("failed to get a bid from relay {relay_index}: {err}");
                    None
                }
            })
            .collect::<Vec<_>>();

        let best_indices = select_best_bids(bids.iter().map(|(bid, i)| (&bid.message.value, *i)));

        if best_indices.is_empty() {
            return Err(Error::NoBids.into());
        }

        // for now, break any ties by picking the first bid,
        // which currently corresponds to the fastest relay
        let (best_index, rest) = best_indices.split_first().unwrap();
        let best_block_hash = &bids[*best_index].0.message.header.block_hash;
        let mut relay_indices = vec![*best_index];
        for index in rest.iter() {
            let block_hash = &bids[*index].0.message.header.block_hash;
            if block_hash == best_block_hash {
                relay_indices.push(*index);
            }
        }

        let mut state = self.state.lock().unwrap();
        let key = BidRequest {
            public_key: Default::default(),
            ..bid_request.clone()
        };
        state.outstanding_bids.insert(key, relay_indices);

        Ok(bids[*best_index].0.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, BlindedBlockProviderError> {
        let relay_indices = {
            let mut state = self.state.lock().unwrap();
            let key = bid_key_from(signed_block);
            match state.outstanding_bids.remove(&key) {
                Some(indices) => indices,
                None => return Err(Error::MissingOpenBid.into()),
            }
        };

        let signed_block = &signed_block;
        let relays = relay_indices.into_iter().map(|i| self.relays[i].clone());
        let responses = stream::iter(relays)
            .map(|relay| async move { relay.open_bid(signed_block).await })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
            .await;

        let expected_block_hash = &signed_block
            .message
            .body
            .execution_payload_header
            .block_hash;
        for (i, response) in responses.into_iter().enumerate() {
            match response {
                Ok(payload) => {
                    if &payload.block_hash == expected_block_hash {
                        return Ok(payload);
                    } else {
                        tracing::warn!("error opening bid from relay {i}: the returned payload did not match the expected block hash: {expected_block_hash}");
                    }
                }
                Err(err) => {
                    tracing::warn!("error opening bid from relay {i}: {err}");
                }
            }
        }

        Err(Error::MissingPayload(expected_block_hash.clone()).into())
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
    use super::*;

    #[test]
    fn test_bid_selection_by_value() {
        let one: U256 = 1.into();
        let two: U256 = 2.into();
        let three: U256 = 3.into();
        let four: U256 = 4.into();

        let test_cases = [
            (vec![], Vec::<usize>::new()),
            (vec![(&one, 0)], vec![0]),
            (vec![(&one, 11), (&one, 22)], vec![11, 22]),
            (vec![(&one, 11), (&two, 22)], vec![22]),
            (vec![(&one, 11), (&two, 22), (&three, 33)], vec![33]),
            (vec![(&two, 22), (&three, 33), (&one, 11)], vec![33]),
            (vec![(&three, 33), (&two, 22), (&one, 11)], vec![33]),
            (
                vec![(&three, 33), (&two, 22), (&three, 44), (&one, 11)],
                vec![33, 44],
            ),
            (
                vec![
                    (&four, 44),
                    (&three, 33),
                    (&two, 22),
                    (&three, 44),
                    (&two, 22),
                    (&two, 22),
                    (&two, 22),
                    (&one, 11),
                ],
                vec![44],
            ),
            (
                vec![
                    (&four, 44),
                    (&four, 45),
                    (&three, 33),
                    (&two, 22),
                    (&three, 44),
                    (&two, 22),
                    (&two, 22),
                    (&two, 22),
                    (&one, 11),
                ],
                vec![44, 45],
            ),
            (
                vec![
                    (&four, 45),
                    (&three, 33),
                    (&two, 22),
                    (&three, 44),
                    (&two, 22),
                    (&two, 22),
                    (&two, 22),
                    (&one, 11),
                    (&four, 44),
                ],
                vec![45, 44],
            ),
            (
                vec![
                    (&three, 33),
                    (&two, 22),
                    (&three, 44),
                    (&two, 22),
                    (&two, 22),
                    (&four, 45),
                    (&two, 22),
                    (&one, 11),
                    (&four, 44),
                ],
                vec![45, 44],
            ),
            (
                vec![
                    (&three, 33),
                    (&two, 22),
                    (&two, 22),
                    (&two, 22),
                    (&two, 22),
                    (&one, 11),
                    (&three, 44),
                    (&four, 45),
                    (&four, 44),
                ],
                vec![45, 44],
            ),
        ];

        for (input, expected) in test_cases.into_iter() {
            let output = select_best_bids(input.into_iter());
            assert_eq!(expected, output);
        }
    }
}
