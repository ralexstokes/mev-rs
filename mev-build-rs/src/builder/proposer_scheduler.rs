use beacon_api_client::{
    BeaconProposerRegistration, Client, Error as ApiClientError, ProposerDuty,
};
use ethereum_consensus::primitives::{Epoch, Slot};
use std::{
    ops::Deref,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::sync::{broadcast, mpsc, oneshot};

#[derive(Debug, Error)]
pub enum Error {
    #[error("api error: {0}")]
    Api(#[from] ApiClientError),
}

// The `ProposerScheduler` ensures the local beacon node is aware
// of validators we manage that it is expected to build for
#[derive(Clone)]
pub struct ProposerScheduler(Arc<Inner>);

impl Deref for ProposerScheduler {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    api: Client,
    slots_per_epoch: Slot,
    _state: Mutex<State>,
}

#[derive(Default)]
struct State {
    // TODO delete this?
}

pub type ProposerSchedule = (
    Vec<ProposerDuty>,
    oneshot::Sender<Vec<BeaconProposerRegistration>>,
);

impl ProposerScheduler {
    pub fn new(api: Client, slots_per_epoch: Slot) -> Self {
        let inner = Inner {
            api,
            slots_per_epoch,
            _state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    async fn dispatch_proposer_preparations(
        &self,
        preparations: Vec<BeaconProposerRegistration>,
    ) -> Result<(), Error> {
        self.api
            .prepare_proposers(&preparations)
            .await
            .map_err(From::from)
    }

    async fn process_duties(
        &self,
        epoch: Epoch,
        proposer_tx: &mpsc::Sender<ProposerSchedule>,
    ) -> Result<(), Error> {
        // TODO be tolerant to re-orgs
        let (_, duties) = self.api.get_proposer_duties(epoch).await?;

        let (tx, rx) = oneshot::channel();
        if let Err(err) = proposer_tx.send((duties, tx)).await {
            tracing::warn!("error sending new duties to builder: {err:?}");
            return Ok(());
        }

        match rx.await {
            Ok(duties_to_dispatch) => {
                self.dispatch_proposer_preparations(duties_to_dispatch)
                    .await
            }
            Err(_) => {
                tracing::warn!("error receiving new duties to watch from builder");
                Ok(())
            }
        }
    }

    pub async fn run(
        &self,
        mut timer: broadcast::Receiver<Slot>,
        proposer_tx: mpsc::Sender<ProposerSchedule>,
        current_epoch: Epoch,
    ) {
        if let Err(err) = self.process_duties(current_epoch, &proposer_tx).await {
            tracing::warn!("error processing incoming duties for epoch {current_epoch}: {err}");
        }
        loop {
            tokio::select! {
                slot = timer.recv() => {
                    match slot {
                        Ok(slot) => {
                            let is_penultimate = slot % self.slots_per_epoch == self.slots_per_epoch - 1;
                            if is_penultimate {
                                let epoch = slot / self.slots_per_epoch;
                                // TODO wait for head in this slot
                                if let Err(err) = self.process_duties(epoch + 1, &proposer_tx).await {
                                    tracing::warn!("error processing incoming duties for epoch {epoch}: {err}");
                                }
                            }
                        }
                        Err(err) => {
                            tracing::warn!("error receiving slot event: {err}");
                        }
                    }

                }
            }
        }
    }
}
