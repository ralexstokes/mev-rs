use async_trait::async_trait;
use ethereum_consensus::{
    primitives::{BlsPublicKey, Slot, U256},
    state_transition::Context,
};
use futures::{stream, StreamExt};
use mev_rs::{
    types::{
        BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
    BlindedBlockProvider, Error,
};
use parking_lot::Mutex;
use rand::prelude::*;
use std::{collections::HashMap, ops::Deref, sync::Arc, time::Duration};

use crate::relay::Relay;

// See note in the `mev-relay-rs::Relay` about this constant.
// TODO likely drop this feature...
const PROPOSAL_TOLERANCE_DELAY: Slot = 1;
// Give relays this amount of time in seconds to return bids.
const FETCH_BEST_BID_TIME_OUT: u64 = 1;

fn validate_bid(
    bid: &mut SignedBuilderBid,
    public_key: &BlsPublicKey,
    context: &Context,
) -> Result<(), Error> {
    if *bid.public_key() != *public_key {
        return Err(Error::BidPublicKeyMismatch {
            bid: bid.public_key().clone(),
            relay: public_key.clone(),
        })
    }
    Ok(bid.verify_signature(context)?)
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

impl Deref for RelayMux {
    type Target = RelayMuxInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct RelayMuxInner {
    relays: Vec<Relay>,
    context: Context,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    // map from bid requests to index of `Relay` in collection
    outstanding_bids: HashMap<BidRequest, Vec<usize>>,
    latest_pubkey: BlsPublicKey,
}

impl RelayMux {
    pub fn new(relays: impl Iterator<Item = Relay>, context: Context) -> Self {
        let inner = RelayMuxInner { relays: relays.collect(), context, state: Default::default() };
        Self(Arc::new(inner))
    }

    pub fn on_slot(&self, slot: Slot) {
        let mut state = self.state.lock();
        state
            .outstanding_bids
            .retain(|bid_request, _| bid_request.slot + PROPOSAL_TOLERANCE_DELAY >= slot);
    }
}

#[async_trait]
impl BlindedBlockProvider for RelayMux {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let registrations = &registrations;
        let responses = stream::iter(self.relays.iter().cloned())
            .map(|relay| async move { relay.register_validators(registrations).await })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
            .await;

        let failures = responses.iter().filter(|r| r.is_err());

        if failures.count() == self.relays.len() {
            Err(Error::CouldNotRegister)
        } else {
            Ok(())
        }
    }

    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error> {
        let responses = stream::iter(self.relays.iter().cloned())
            .map(|relay| async move {
                tokio::time::timeout(
                    Duration::from_secs(FETCH_BEST_BID_TIME_OUT),
                    relay.fetch_best_bid(bid_request),
                )
                .await
            })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
            .await;

        // ideally can fuse the filtering into the prior async fetch but
        // several attempts lead to opaque compiler errors...
        let bids = responses
        .into_iter()
        .enumerate()
        .filter_map(|(relay_index, response)| match response {
            Ok(Ok(mut bid)) => {
                if let Err(err) = validate_bid(&mut bid, &self.relays[relay_index].public_key, &self.context) {
                    tracing::warn!("invalid signed builder bid: {err} for bid: {bid:?}");
                    None
                } else {
                    Some((bid, relay_index))
                }
            }
            Ok(Err(err)) => {
                tracing::warn!("failed to get a bid from relay {relay_index}: {err}");
                None
            }
            Err(_) => {
                tracing::warn!(
                    "relay {relay_index} didn't provide bid before time out {FETCH_BEST_BID_TIME_OUT}s."
                );
                None
            }
        })
        .collect::<Vec<_>>();

        let mut best_indices = select_best_bids(bids.iter().map(|(bid, i)| (bid.value(), *i)));

        if best_indices.is_empty() {
            return Err(Error::NoBids)
        }

        // if multiple indices with same bid value, break tie by randomly picking one
        let mut rng = rand::thread_rng();
        best_indices.shuffle(&mut rng);
        let (best_index, rest) = best_indices.split_first().unwrap();
        let best_block_hash = &bids[*best_index].0.block_hash();
        let mut relay_indices = vec![*best_index];
        for index in rest {
            let block_hash = &bids[*index].0.block_hash();
            if block_hash == best_block_hash {
                relay_indices.push(*index);
            }
        }

        {
            let mut state = self.state.lock();
            // assume the next request to open a bid corresponds to the current request
            // TODO consider if the relay mux should have more knowledge about the proposal
            state.latest_pubkey = bid_request.public_key.clone();
            state.outstanding_bids.insert(bid_request.clone(), relay_indices);
        }

        let best_bid = bids[*best_index].0.clone();
        Ok(best_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let relay_indices = {
            let mut state = self.state.lock();
            let key = bid_key_from(signed_block, &state.latest_pubkey);
            state.outstanding_bids.remove(&key).ok_or(Error::MissingOpenBid)?
        };

        let signed_block = &signed_block;
        let relays = relay_indices.into_iter().map(|i| self.relays[i].clone());
        let responses = stream::iter(relays)
            .map(|relay| async move { relay.open_bid(signed_block).await })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
            .await;

        let expected_block_hash = signed_block.block_hash();
        for (i, response) in responses.into_iter().enumerate() {
            match response {
                Ok(payload) => {
                    let block_hash = payload.block_hash();
                    if block_hash == expected_block_hash {
                        return Ok(payload)
                    } else {
                        tracing::warn!("error opening bid from relay {i}: the returned payload with block hash {block_hash} did not match the expected block hash: {expected_block_hash}");
                    }
                }
                Err(err) => {
                    tracing::warn!("error opening bid from relay {i}: {err}");
                }
            }
        }

        Err(Error::MissingPayload(expected_block_hash.clone()))
    }
}

fn bid_key_from(signed_block: &SignedBlindedBeaconBlock, public_key: &BlsPublicKey) -> BidRequest {
    let slot = signed_block.slot();
    let parent_hash = signed_block.parent_hash().clone();

    BidRequest { slot, parent_hash, public_key: public_key.clone() }
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
            (vec![(&three, 33), (&two, 22), (&three, 44), (&one, 11)], vec![33, 44]),
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
