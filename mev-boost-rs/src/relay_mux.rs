use async_trait::async_trait;
use ethereum_consensus::{
    crypto::KzgCommitment,
    primitives::{BlsPublicKey, Hash32, Slot, U256},
    state_transition::Context,
};
use futures_util::{stream, StreamExt};
use mev_rs::{
    relay::Relay,
    signing::verify_signed_builder_data,
    types::{
        AuctionContents, AuctionRequest, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
    BlindedBlockProvider, BoostError, Error,
};
use parking_lot::Mutex;
use rand::prelude::*;
use std::{cmp::Ordering, collections::HashMap, ops::Deref, sync::Arc, time::Duration};
use tokio::time::timeout;
use tracing::{debug, info, warn};

// Track an auction for this amount of time, in slots.
const AUCTION_LIFETIME: u64 = 2;
// Give relays this amount of time in seconds to process validator registrations.
const VALIDATOR_REGISTRATION_TIME_OUT_SECS: u64 = 4;
// Give relays this amount of time in seconds to return bids.
const FETCH_BEST_BID_TIME_OUT_SECS: u64 = 1;
// Give relays this amount of time in seconds to respond with a payload.
const FETCH_PAYLOAD_TIME_OUT_SECS: u64 = 4;

#[derive(Debug)]
struct AuctionContext {
    slot: Slot,
    relays: Vec<Arc<Relay>>,
}

fn validate_bid(
    bid: &SignedBuilderBid,
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
    verify_signed_builder_data(&bid.message, public_key, &bid.signature, context)
        .map_err(Into::into)
}

fn validate_payload(
    contents: &AuctionContents,
    expected_block_hash: &Hash32,
    expected_commitments: Option<&[KzgCommitment]>,
) -> Result<(), BoostError> {
    let provided_block_hash = contents.execution_payload().block_hash();
    if expected_block_hash != provided_block_hash {
        return Err(BoostError::InvalidPayloadHash {
            expected: expected_block_hash.clone(),
            provided: provided_block_hash.clone(),
        })
    }
    let provided_commitments = contents.blobs_bundle().map(|bundle| &bundle.commitments);
    match (expected_commitments, provided_commitments) {
        (Some(expected), Some(provided)) => {
            if expected == provided.as_ref() {
                Ok(())
            } else {
                Err(BoostError::InvalidPayloadBlobs {
                    expected: expected.to_vec(),
                    provided: provided.to_vec(),
                })
            }
        }
        (None, None) => Ok(()),
        _ => Err(BoostError::InvalidPayloadUnexpectedBlobs),
    }
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
    context: Arc<Context>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    outstanding_bids: HashMap<Hash32, Arc<AuctionContext>>,
}

impl RelayMux {
    pub fn new(relays: Vec<Relay>, context: Arc<Context>) -> Self {
        let inner = Inner {
            relays: relays.into_iter().map(Arc::new).collect(),
            context,
            state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    pub fn on_slot(&self, slot: Slot) {
        debug!(slot, "processing");
        let retain_slot = slot.checked_sub(AUCTION_LIFETIME).unwrap_or_default();
        let mut state = self.state.lock();
        state.outstanding_bids.retain(|_, auction| auction.slot >= retain_slot);
    }

    fn get_context(&self, key: &Hash32) -> Result<Arc<AuctionContext>, Error> {
        let state = self.state.lock();
        state
            .outstanding_bids
            .get(key)
            .cloned()
            .ok_or_else::<Error, _>(|| BoostError::MissingOpenBid(key.clone()).into())
    }
}

#[async_trait]
impl BlindedBlockProvider for RelayMux {
    async fn register_validators(
        &self,
        registrations: &[SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let responses = stream::iter(self.relays.iter().cloned())
            .map(|relay| async {
                let request = relay.register_validators(registrations);
                let duration = Duration::from_secs(VALIDATOR_REGISTRATION_TIME_OUT_SECS);
                let result = timeout(duration, request).await;
                (relay, result)
            })
            .buffer_unordered(self.relays.len())
            .filter_map(|(relay, result)| async move {
                match result {
                    Ok(Ok(_)) => Some(()),
                    Ok(Err(err)) => {
                        warn!(%err, %relay, "failure when registering validator(s)");
                        None
                    }
                    Err(_) => {
                        warn!(%relay, "timeout when registering validator(s)");
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
            .await;

        if responses.is_empty() {
            Err(BoostError::CouldNotRegister.into())
        } else {
            let count = registrations.len();
            info!(count, "sent validator registrations");
            Ok(())
        }
    }

    async fn fetch_best_bid(
        &self,
        auction_request: &AuctionRequest,
    ) -> Result<SignedBuilderBid, Error> {
        let bids = stream::iter(self.relays.iter().cloned())
            .map(|relay| async {
                let request = relay.fetch_best_bid(auction_request);
                let duration = Duration::from_secs(FETCH_BEST_BID_TIME_OUT_SECS);
                let result = timeout(duration, request).await;
                (relay, result)
            })
            .buffer_unordered(self.relays.len())
            .filter_map(|(relay, result)| async {
                match result {
                    Ok(Ok(bid)) => {
                        if let Err(err) = validate_bid(&bid, &relay.public_key, &self.context) {
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

        let slot = auction_request.slot;
        info!(
            slot,
            parent_hash = ?auction_request.parent_hash,
            public_key = ?auction_request.public_key,
            %best_bid,
            relays = ?best_relays,
            "acquired best bid"
        );

        {
            let mut state = self.state.lock();
            let auction_context = AuctionContext { slot, relays: best_relays };
            state.outstanding_bids.insert(best_block_hash.clone(), Arc::new(auction_context));
        }

        Ok(best_bid.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<AuctionContents, Error> {
        let block = signed_block.message();
        let slot = block.slot();
        let body = block.body();
        let expected_block_hash = body.execution_payload_header().block_hash().clone();
        let context = self.get_context(&expected_block_hash)?;

        let responses = stream::iter(context.relays.iter().cloned())
            .map(|relay| async move {
                let request = relay.open_bid(signed_block);
                let duration = Duration::from_secs(FETCH_PAYLOAD_TIME_OUT_SECS);
                let result = timeout(duration, request).await;
                (relay, result)
            })
            .buffer_unordered(self.relays.len())
            .filter_map(|(relay, result)| async move {
                match result {
                    Ok(response) => Some((relay, response)),
                    Err(_) => {
                        warn!( %relay, "timeout when opening bid");
                        None
                    }
                }
            })
            .collect::<Vec<_>>()
            .await;

        for (relay, response) in responses.into_iter() {
            match response {
                Ok(auction_contents) => match validate_payload(
                    &auction_contents,
                    &expected_block_hash,
                    body.blob_kzg_commitments().map(|commitments| commitments.as_slice()),
                ) {
                    Ok(_) => {
                        info!(%slot, block_hash = %expected_block_hash, %relay, "acquired payload");
                        return Ok(auction_contents)
                    }
                    Err(err) => {
                        warn!(?err, ?relay, "could not validate payload");
                    }
                },
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
