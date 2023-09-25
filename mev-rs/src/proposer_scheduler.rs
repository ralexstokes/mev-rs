use beacon_api_client::{
    mainnet::Client, BeaconProposerRegistration, Error as ApiError, ProposerDuty, PublicKeyOrIndex,
    StateId,
};
use ethereum_consensus::{
    builder::SignedValidatorRegistration,
    primitives::{BlsPublicKey, Epoch, Hash32, Slot, ValidatorIndex},
};
use parking_lot::Mutex;
use std::collections::HashMap;
use thiserror::Error;

use crate::ValidatorRegistry;

pub type Coordinate = (Slot, Hash32, BlsPublicKey);
pub type Proposal = (ValidatorIndex, BlsPublicKey, SignedValidatorRegistration);

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
    proposer_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
}

impl ProposerScheduler {
    pub fn new(api: Client) -> Self {
        Self { api, state: Mutex::new(State::default()) }
    }

    pub async fn dispatch_proposer_preparations(
        &self,
        preparations: &[BeaconProposerRegistration],
    ) -> Result<(), Error> {
        self.api.prepare_proposers(preparations).await.map_err(From::from)
    }

    // fetch proposer schedule
    pub async fn fetch_duties(
        &self,
        epoch: Epoch,
        registry: &ValidatorRegistry,
    ) -> Result<Vec<ProposerDuty>, Error> {
        // TODO be tolerant to re-orgs
        let (_dependent_root, duties) = self.api.get_proposer_duties(epoch).await?;
        // index an internal copy of validator preferences
        let preferences = registry.state.lock().validator_preferences.clone();
        let state = &mut self.state.lock();
        state.proposer_preferences = preferences;

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

    pub async fn get_proposal(&self, coordinate: Coordinate) -> Result<Proposal, Error> {
        let (_slot, _parent_hash, public_key) = coordinate;
        let summary = self
            .api
            .get_validator(StateId::Head, PublicKeyOrIndex::PublicKey(public_key.clone()))
            .await?;
        let validator_index = summary.index;

        let registration =
            self.state.lock().proposer_preferences.get(&public_key).cloned().unwrap();

        Ok((validator_index, public_key, registration))
    }
}
