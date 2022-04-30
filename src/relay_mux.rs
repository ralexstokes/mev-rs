use crate::relay::{Relay, RelayError};
use crate::types::{
    BuilderBidV1, ExecutionPayload, ProposalRequest, SignedBlindedBeaconBlock,
    ValidatorRegistrationV1,
};
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
    #[error("could not find relay for block")]
    MissingProposal,
    #[error("issue with relay: {0}")]
    Relay(#[from] RelayError),
}

#[derive(Clone)]
pub struct RelayMux(Arc<RelayMuxInner>);

struct RelayMuxInner {
    relays: Vec<Relay>,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    // map from proposals to index of `Relay` in collection
    outstanding_bids: HashMap<ProposalRequest, usize>,
}

impl RelayMux {
    pub fn new(relays: Vec<Relay>) -> Self {
        let state = State {
            outstanding_bids: HashMap::new(),
        };
        let inner = RelayMuxInner {
            relays,
            state: Mutex::new(state),
        };
        Self(Arc::new(inner))
    }

    // tmp
    pub async fn run(&self) {
        let mut interval = time::interval(Duration::from_secs(12));
        loop {
            interval.tick().await;
            let state = self.0.state.lock().unwrap();
            // TODO purge expired state if a proposal fails for some reason
            dbg!(state);
        }
    }

    pub async fn register_validator(
        &self,
        registration: &ValidatorRegistrationV1,
    ) -> Vec<Result<(), RelayError>> {
        join_all(
            self.0
                .relays
                .iter()
                .map(|relay| relay.register_validator(registration)),
        )
        .await
    }

    pub async fn fetch_best_bid(
        &self,
        proposal_request: &ProposalRequest,
    ) -> Result<BuilderBidV1, Error> {
        // TODO do not block on slow relays
        let bids = join_all(
            self.0
                .relays
                .iter()
                .enumerate()
                .map(|(i, relay)| async move { (i, relay.fetch_bid(proposal_request).await) }),
        )
        .await;

        let (relay_index, best_bid) = bids
            .into_iter()
            .filter_map(|(i, bid)| match bid {
                Ok(bid) => Some((i, bid)),
                Err(err) => {
                    tracing::warn!("{err}");
                    None
                }
            })
            .max_by_key(|(_, bid)| bid.value)
            .ok_or_else(|| Error::NoBidsReturned)?;

        let mut state = self.0.state.lock().unwrap();
        state
            .outstanding_bids
            .insert(proposal_request.clone(), relay_index);

        Ok(best_bid)
    }

    pub async fn accept_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let relay_index = {
            let state = self.0.state.lock().unwrap();
            let key = proposal_from(signed_block);
            match state.outstanding_bids.get(&key) {
                Some(relay_index) => *relay_index,
                None => return Err(Error::MissingProposal),
            }
        };

        let relay = &self.0.relays[relay_index];
        Ok(relay.accept_bid(signed_block).await?)
    }
}

fn proposal_from(signed_block: &SignedBlindedBeaconBlock) -> ProposalRequest {
    // TODO: fill out once types exist
    // let block = &signed_block.message;
    ProposalRequest { a: 122 }
}
