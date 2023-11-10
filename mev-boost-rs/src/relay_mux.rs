use async_trait::async_trait;
use ethereum_consensus::{
    primitives::{BlsPublicKey, Epoch, Slot, U256},
    state_transition::Context,
};
use futures::{stream, StreamExt};
use mev_rs::{
    relay::Relay,
    types::{
        AuctionContents, AuctionRequest, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
    BlindedBlockProvider, BoostError, Error,
};
use parking_lot::Mutex;
use rand::prelude::*;
use std::{cmp::Ordering, collections::HashMap, ops::Deref, sync::Arc, time::Duration};
use tracing::{debug, info, warn};

// See note in the `mev-relay-rs::Relay` about this constant.
// TODO likely drop this feature...
const PROPOSAL_TOLERANCE_DELAY: Slot = 1;
// Give relays this amount of time in seconds to return bids.
const FETCH_BEST_BID_TIME_OUT_SECS: u64 = 1;

fn bid_key_from(
    signed_block: &SignedBlindedBeaconBlock,
    public_key: &BlsPublicKey,
) -> AuctionRequest {
    let slot = signed_block.message().slot();
    let parent_hash =
        signed_block.message().body().execution_payload_header().parent_hash().clone();

    AuctionRequest { slot, parent_hash, public_key: public_key.clone() }
}

fn validate_bid(
    bid: &mut SignedBuilderBid,
    public_key: &BlsPublicKey,
    context: &Context,
) -> Result<(), Error> {
    let bid_public_key = bid.message.public_key();
    if bid_public_key != public_key {
        return Err(BoostError::BidPublicKeyMismatch {
            bid: bid_public_key.clone(),
            relay: public_key.clone(),
        }
        .into())
    }
    Ok(bid.verify_signature(context)?)
}

// Select the most valuable bids in `bids`, breaking ties by `block_hash`
fn select_best_bids(bids: impl Iterator<Item = (usize, U256)>) -> Vec<usize> {
    let (best_indices, _value) =
        bids.fold((vec![], U256::ZERO), |(mut best_indices, max), (index, value)| {
            match value.cmp(&max) {
                Ordering::Greater => (vec![index], value),
                Ordering::Equal => {
                    best_indices.push(index);
                    (best_indices, max)
                }
                Ordering::Less => (best_indices, max),
            }
        });
    best_indices
}

#[derive(Clone)]
pub struct RelayMux(Arc<Inner>);

impl Deref for RelayMux {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    relays: Vec<Arc<Relay>>,
    context: Context,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    // map from bid requests to index of `Relay` in collection
    outstanding_bids: HashMap<AuctionRequest, Vec<Arc<Relay>>>,
    current_epoch_registration_count: usize,
    latest_pubkey: BlsPublicKey,
}

impl RelayMux {
    pub fn new(relays: impl Iterator<Item = Relay>, context: Context) -> Self {
        let inner =
            Inner { relays: relays.map(Arc::new).collect(), context, state: Default::default() };
        Self(Arc::new(inner))
    }

    pub fn on_slot(&self, slot: Slot) {
        debug!(slot, "processing");
        let mut state = self.state.lock();
        state
            .outstanding_bids
            .retain(|auction_request, _| auction_request.slot + PROPOSAL_TOLERANCE_DELAY >= slot);
    }

    pub fn on_epoch(&self, epoch: Epoch) {
        debug!(epoch, "processing");
        let count = {
            let mut state = self.state.lock();
            let count = state.current_epoch_registration_count;
            state.current_epoch_registration_count = 0;
            count
        };
        info!(count, epoch, "processed validator registrations")
    }
}

#[async_trait]
impl BlindedBlockProvider for RelayMux {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let responses = stream::iter(self.relays.iter().cloned())
            .map(|relay| async {
                let response = relay.register_validators(registrations).await;
                (relay, response)
            })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
            .await;

        let mut num_failures = 0;
        for (relay, response) in responses {
            if let Err(err) = response {
                num_failures += 1;
                warn!(%relay, %err, "failed to register validator");
            }
        }

        if num_failures == self.relays.len() {
            Err(BoostError::CouldNotRegister.into())
        } else {
            let count = registrations.len();
            info!(count, "sent validator registrations");
            let mut state = self.state.lock();
            state.current_epoch_registration_count += registrations.len();
            Ok(())
        }
    }

    async fn fetch_best_bid(
        &self,
        auction_request: &AuctionRequest,
    ) -> Result<SignedBuilderBid, Error> {
        let bids = stream::iter(self.relays.iter().cloned())
            .map(|relay| async {
                let response = tokio::time::timeout(
                    Duration::from_secs(FETCH_BEST_BID_TIME_OUT_SECS),
                    relay.fetch_best_bid(auction_request),
                )
                .await;
            (relay, response)
           })
            .buffer_unordered(self.relays.len())
            .filter_map(|(relay, response)| async {
                match response {
                    Ok(Ok(mut bid)) => {
                        if let Err(err) = validate_bid(&mut bid, &relay.public_key, &self.context) {
                            warn!(%err, %relay, "invalid signed builder bid");
                            None
                        } else {
                            Some((relay, bid))
                        }
                    }
                    Ok(Err(Error::NoBidPrepared(auction_request))) => {
                        debug!(%auction_request, %relay, "relay did not have a bid prepared");
                        None
                    }
                    Ok(Err(err)) => {
                        warn!(%err, %relay, "failed to get a bid");
                        None
                    }
                    Err(_) => {
                        warn!(timeout_in_sec = FETCH_BEST_BID_TIME_OUT_SECS, %relay, "timeout when fetching bid");
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
            .await;

        if bids.is_empty() {
            info!(%auction_request, "no relays had bids prepared");
            return Err(Error::NoBidPrepared(auction_request.clone()))
        }

        // TODO: change `value` so it does the copy internally
        let mut best_bid_indices =
            select_best_bids(bids.iter().map(|(_, bid)| bid.message.value()).enumerate());

        // if multiple distinct bids with same bid value, break tie by randomly picking one
        let mut rng = rand::thread_rng();
        best_bid_indices.shuffle(&mut rng);

        let (best_bid_index, rest) =
            best_bid_indices.split_first().expect("there is at least one bid");

        let (best_relay, best_bid) = &bids[*best_bid_index];
        let best_block_hash = best_bid.message.header().block_hash();

        let mut best_relays = vec![best_relay.clone()];
        for bid_index in rest {
            let (relay, bid) = &bids[*bid_index];
            if bid.message.header().block_hash() == best_block_hash {
                best_relays.push(relay.clone());
            }
        }

        let relays_desc = best_relays
            .iter()
            .map(|relay| format!("{relay}"))
            .reduce(|desc, next| format!("{desc}, {next}"))
            .expect("at least one relay");
        info!(%auction_request, %best_bid, relays=relays_desc, "acquired best bid");

        {
            let mut state = self.state.lock();
            // assume the next request to open a bid corresponds to the current request
            // TODO consider if the relay mux should have more knowledge about the proposal
            state.latest_pubkey = auction_request.public_key.clone();
            state.outstanding_bids.insert(auction_request.clone(), best_relays);
        }

        Ok(best_bid.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<AuctionContents, Error> {
        let (auction_request, relays) = {
            let mut state = self.state.lock();
            let key = bid_key_from(signed_block, &state.latest_pubkey);
            // TODO: do not `remove` so this endpoint can be retried
            let relays = state
                .outstanding_bids
                .remove(&key)
                .ok_or_else::<Error, _>(|| BoostError::MissingOpenBid.into())?;
            (key, relays)
        };

        let signed_block = &signed_block;
        let responses = stream::iter(relays)
            .map(|relay| {
                let signed_block = signed_block.clone(); 
                async move {
                    let response = tokio::time::timeout(
                        Duration::from_secs(FETCH_BEST_BID_TIME_OUT_SECS),
                        relay.open_bid(&signed_block),
                    )
                    .await;
                    (relay, response)
                }
            })
            .buffer_unordered(self.relays.len())
            .collect::<Vec<_>>()
            .await;

        let block = signed_block.message();
        let block_body = block.body();
        let payload_header = block_body.execution_payload_header();
        let expected_block_hash = payload_header.block_hash();
        for (relay, response) in responses.into_iter() {
            match response {
                Ok(auction_contents) => {
                    let block_hash = auction_contents.execution_payload().block_hash();
                    if block_hash == expected_block_hash {
                        info!(%auction_request, %block_hash, %relay, "acquired payload");
                        return Ok(auction_contents)
                    } else {
                        warn!(?block_hash, ?expected_block_hash, %relay, "incorrect block hash delivered by relay");
                    }
                }
                Err(err) => {
                    warn!(%err, %relay, "error opening bid");
                }
            }
        }

        Err(BoostError::MissingPayload(expected_block_hash.clone()).into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bid_selection_by_value() {
        let test_cases = [
            (vec![], Vec::<usize>::new()),
            (vec![1], vec![0]),
            (vec![1, 1], vec![0, 1]),
            (vec![1, 2], vec![1]),
            (vec![1, 2, 3], vec![2]),
            (vec![2, 3, 1], vec![1]),
            (vec![3, 2, 1], vec![0]),
            (vec![3, 2, 3, 1], vec![0, 2]),
            (vec![4, 3, 2, 3, 2, 2, 2, 1], vec![0]),
            (vec![4, 4, 3, 2, 3, 2, 2, 2, 1], vec![0, 1]),
            (vec![4, 3, 2, 3, 2, 2, 2, 1, 4], vec![0, 8]),
            (vec![3, 2, 3, 2, 2, 4, 2, 1, 4], vec![5, 8]),
            (vec![3, 2, 2, 2, 2, 1, 3, 4, 4], vec![7, 8]),
        ];

        for (mut input, expected) in test_cases.into_iter() {
            let best_bid_indices =
                select_best_bids(input.iter().map(|x| U256::from(*x)).enumerate());
            assert_eq!(expected, best_bid_indices);

            if best_bid_indices.is_empty() {
                continue
            }

            // NOTE: test randomization logic
            let mut rng = rand::thread_rng();
            input.shuffle(&mut rng);
            let (best_index, _) = best_bid_indices.split_first().unwrap();
            assert!(input.get(*best_index).is_some());
        }
    }
}
