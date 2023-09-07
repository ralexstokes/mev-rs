use beacon_api_client::{
    mainnet::Client, BeaconProposerRegistration, Error as ApiError, ProposerDuty,
};
use ethereum_consensus::primitives::{BlsPublicKey, Epoch, Slot};
use parking_lot::Mutex;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("api error: {0}")]
    Api(#[from] ApiError),
}

pub struct ProposerScheduler {
    api: Client,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    proposer_schedule: HashMap<Slot, BlsPublicKey>,
}

impl ProposerScheduler {
    pub fn new(api: Client) -> Self {
        Self { api, state: Default::default() }
    }

    pub async fn dispatch_proposer_preparations(
        &self,
        preparations: &[BeaconProposerRegistration],
    ) -> Result<(), Error> {
        self.api.prepare_proposers(preparations).await.map_err(From::from)
    }

    pub async fn fetch_duties(&self, epoch: Epoch) -> Result<Vec<ProposerDuty>, Error> {
        // TODO be tolerant to re-orgs
        let (_dependent_root, duties) = self.api.get_proposer_duties(epoch).await?;
        let mut state = self.state.lock();
        for duty in &duties {
            let slot = duty.slot;
            let public_key = &duty.public_key;
            state.proposer_schedule.insert(slot, public_key.clone());
        }
        Ok(duties)
    }

    pub fn get_proposer_for(&self, slot: Slot) -> Option<BlsPublicKey> {
        let state = self.state.lock();
        state.proposer_schedule.get(&slot).cloned()
    }
}
