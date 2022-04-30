use crate::relay::{Message as RelayMessage, RelayError};
use crate::types::{
    BuilderBidV1, ExecutionPayload, ProposalRequest, SignedBlindedBeaconBlock,
    ValidatorRegistrationV1,
};
use futures::future::join_all;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tokio::sync::oneshot;
use tokio::time;

type Relay = Sender<RelayMessage>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no bids returned for proposal")]
    NoBidsReturned,
    #[error("issue with relay: {0}")]
    Relay(#[from] RelayError),
}

#[derive(Clone)]
pub struct RelayMux(Arc<RelayMuxInner>);

struct RelayMuxInner {
    relays: Vec<Sender<RelayMessage>>,
    state: Mutex<State>,
}

#[derive(Debug)]
struct State {
    outstanding_bids: HashMap<ProposalRequest, BuilderBidV1>,
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

    pub async fn run(&self) {
        let mut interval = time::interval(Duration::from_secs(1));
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
        let responses = join_all(self.0.relays.iter().map(|relay| async {
            let (resp_tx, resp_rx) = oneshot::channel();
            let msg = RelayMessage::Registration(registration.clone(), resp_tx);
            let _ = relay.send(msg).await;
            resp_rx.await
        }))
        .await;

        for response in responses {
            tracing::info!("{response:?}");
        }
        vec![]
    }

    pub async fn fetch_best_bid(
        &self,
        proposal_request: &ProposalRequest,
    ) -> Result<BuilderBidV1, Vec<Error>> {
        let responses = join_all(self.0.relays.iter().map(|relay| async {
            let (resp_tx, resp_rx) = oneshot::channel();
            let msg = RelayMessage::FetchBid(proposal_request.clone(), resp_tx);
            let _ = relay.send(msg).await;
            resp_rx.await?
        }))
        .await;

        // TODO do not block on slow relays
        let best_bid = responses
            .into_iter()
            .filter_map(|result| match result {
                Ok(bid) => Some(bid),
                Err(err) => {
                    tracing::warn!("{err}");
                    None
                }
            })
            .max_by_key(|bid| bid.value)
            .ok_or_else(|| vec![Error::NoBidsReturned])?;

        // TODO track by relay
        let mut state = self.0.state.lock().unwrap();
        state
            .outstanding_bids
            .insert(proposal_request.clone(), best_bid.clone());

        Ok(best_bid)
    }

    pub async fn accept_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Vec<RelayError>> {
        // TODO: post the block
        // TODO: return the execution payload
        Ok(ExecutionPayload { a: 12 })
    }
}
