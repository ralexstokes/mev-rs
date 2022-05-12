use crate::relay::Relay;
use crate::types::{
    BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
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
    #[error("issue with relay: {0}")]
    Relay(#[from] ApiError),
}

#[derive(Clone)]
pub struct RelayMux(Arc<RelayMuxInner>);

struct RelayMuxInner {
    relays: Vec<Relay>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    // map from bid requests to index of `Relay` in collection
    outstanding_bids: HashMap<BidRequest, usize>,
}

impl RelayMux {
    pub fn new(relays: Vec<Relay>) -> Self {
        let inner = RelayMuxInner {
            relays,
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

    pub async fn register_validator(
        &self,
        registration: &SignedValidatorRegistration,
    ) -> Result<(), Error> {
        let responses = join_all(self.0.relays.iter().map(|relay| async {
            relay
                .register_validator(registration)
                .await
                .map_err(Error::from)
        }))
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
            Err(Error::CouldNotRegister)
        }
    }

    pub async fn fetch_best_bid(
        &self,
        bid_request: &BidRequest,
    ) -> Result<SignedBuilderBid, Error> {
        // TODO do not block on slow relays
        let bids = join_all(
            self.0
                .relays
                .iter()
                .enumerate()
                .map(|(i, relay)| async move { (i, relay.fetch_bid(bid_request).await) }),
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

        let mut state = self.0.state.lock().unwrap();
        state
            .outstanding_bids
            .insert(bid_request.clone(), relay_index);

        Ok(best_bid)
    }

    pub async fn accept_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let relay_index = {
            let mut state = self.0.state.lock().unwrap();
            let key = bid_request_from(signed_block);
            match state.outstanding_bids.remove(&key) {
                Some(relay_index) => relay_index,
                None => return Err(Error::MissingOpenBid),
            }
        };

        let relay = &self.0.relays[relay_index];
        Ok(relay.accept_bid(signed_block).await?)
    }
}

fn bid_request_from(signed_block: &SignedBlindedBeaconBlock) -> BidRequest {
    let block = &signed_block.message;

    // TODO: index -> pubkey
    let public_key = Default::default();

    BidRequest {
        slot: block.slot,
        public_key,
        parent_hash: block.body.execution_payload_header.parent_hash.clone(),
    }
}
