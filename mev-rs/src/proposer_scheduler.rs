use crate::{types::ProposerSchedule, validator_registry::ValidatorRegistry};
use beacon_api_client::{Error as ApiError, ProposerDuty};
use ethereum_consensus::primitives::{Epoch, Slot};
use parking_lot::Mutex;
use thiserror::Error;
use tracing::warn;

#[cfg(not(feature = "minimal-preset"))]
use beacon_api_client::mainnet::Client;
#[cfg(feature = "minimal-preset")]
use beacon_api_client::minimal::Client;

#[derive(Debug, Error)]
pub enum Error {
    #[error("api error: {0}")]
    Api(#[from] ApiError),
}

pub struct ProposerScheduler {
    api: Client,
    slots_per_epoch: Slot,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    // schedules are monotonically increasing by `slot`
    // but may not be contiguous as schedules are created only
    // if we have a valid registration from the proposer
    proposer_schedule: Vec<ProposerSchedule>,
}

impl ProposerScheduler {
    pub fn new(api: Client, slots_per_epoch: Slot) -> Self {
        Self { api, slots_per_epoch, state: Default::default() }
    }

    async fn fetch_duties_if_missing(
        &self,
        epoch: Epoch,
        all_duties: &mut Vec<ProposerDuty>,
    ) -> Result<(), Error> {
        {
            let slot = epoch * self.slots_per_epoch;
            let state = self.state.lock();
            if state.proposer_schedule.iter().any(|schedule| schedule.slot >= slot) {
                return Ok(());
            }
        }
        // TODO be tolerant to re-orgs
        let (_dependent_root, duties) = self.api.get_proposer_duties(epoch).await?;
        all_duties.extend(duties);
        Ok(())
    }

    // Fetches proposer duties for the current epoch `epoch` and the next epoch.
    async fn fetch_new_duties(&self, epoch: Epoch) -> Vec<ProposerDuty> {
        let mut duties = vec![];
        if let Err(err) = self.fetch_duties_if_missing(epoch, &mut duties).await {
            warn!(%err, epoch, "could not get proposer duties from consensus");
        }
        if let Err(err) = self.fetch_duties_if_missing(epoch + 1, &mut duties).await {
            warn!(%err, epoch = epoch + 1, "could not get proposer duties from consensus");
        }
        duties
    }

    pub async fn on_epoch(
        &self,
        epoch: Epoch,
        validator_registry: &ValidatorRegistry,
    ) -> Result<(), Error> {
        let extension = self
            .fetch_new_duties(epoch)
            .await
            .iter()
            .filter_map(|duty| {
                let public_key = &duty.public_key;
                validator_registry.get_signed_registration(public_key).map(|entry| {
                    ProposerSchedule {
                        slot: duty.slot,
                        validator_index: duty.validator_index,
                        entry: entry.clone(),
                    }
                })
            })
            // collect so we do the work *before* grabbing the lock
            .collect::<Vec<_>>();

        let slot = epoch * self.slots_per_epoch;
        let mut state = self.state.lock();
        // drop old schedules
        state.proposer_schedule.retain(|schedule| schedule.slot >= slot);
        // add new schedules
        state.proposer_schedule.extend(extension);
        Ok(())
    }

    pub fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error> {
        // NOTE: if external APIs hold, then the expected schedules are
        // those currently in the `state`.
        let state = self.state.lock();
        Ok(state.proposer_schedule.clone())
    }
}
