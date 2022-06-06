use beacon_api_client::Client;
use ethereum_consensus::primitives::{BlsPublicKey, ExecutionAddress, Slot, ValidatorIndex};
use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};
use tokio::sync::{broadcast, mpsc, oneshot};

#[derive(Clone)]
pub struct ProposerScheduler(Arc<Inner>);

impl Deref for ProposerScheduler {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    timer: broadcast::Receiver<Slot>,
    proposer_tx: mpsc::Sender<ProposerSchedule>,
    api: Client,
    slots_per_epoch: Slot,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {}

pub type ProposerPreparation = (ValidatorIndex, ExecutionAddress);

pub type ProposerSchedule = (Vec<BlsPublicKey>, oneshot::Sender<Vec<ProposerPreparation>>);

impl ProposerScheduler {
    pub fn new(
        timer: broadcast::Receiver<Slot>,
        proposer_tx: mpsc::Sender<ProposerSchedule>,
        api: Client,
        slots_per_epoch: Slot,
    ) -> Self {
        let inner = Inner {
            timer,
            proposer_tx,
            api,
            slots_per_epoch,
            state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    pub async fn run(&mut self) {
        // comp: duties manager
        // INIT:
        // get duties for *current* epoch
        // put into scheduler
        // LOOP:
        // at penultimate slot of epoch, wait for head
        // then ask for duties of *next* epoch
        // put into scheduler
        // TODO: be tolerant to shuffling changes

        // comp: scheduler for "prepare" dispatch
        // LOOP: on each slot
        // check if duty for next slot
        // -- WANT to block until the duty exists
        // then *IFF* we have a registration for that proposer
        // call validator `prepare_beacon_proposer` with the fee recip
    }

    pub fn get_proposer_in_slot(&self, slot: Slot) -> BlsPublicKey {
        Default::default()
    }

    pub fn prepare_proposer(&self) {
        // dispatch `prepare_beacon_proposer` to BN
    }
}
