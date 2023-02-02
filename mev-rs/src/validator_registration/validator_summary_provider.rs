use beacon_api_client::{Client, Error as ApiError, StateId, ValidatorStatus, ValidatorSummary};
use ethereum_consensus::primitives::{BlsPublicKey, ValidatorIndex};
use parking_lot::Mutex;
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("missing knowledge of pubkey in validator set")]
    UnknownPubkey,
    #[error("missing knowledge of index in validator set")]
    UnknownIndex,
}

pub struct ValidatorSummaryProvider {
    client: Client,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    validators: HashMap<BlsPublicKey, ValidatorSummary>,
    pubkeys_by_index: HashMap<ValidatorIndex, BlsPublicKey>,
}

impl ValidatorSummaryProvider {
    pub fn new(client: Client) -> Self {
        let state = State::default();
        Self { client, state: Mutex::new(state) }
    }

    pub async fn load(&self) -> Result<(), Error> {
        let summaries = self.client.get_validators(StateId::Head, &[], &[]).await?;
        let mut state = self.state.lock();
        for summary in summaries.into_iter() {
            let public_key = summary.validator.public_key.clone();
            state.pubkeys_by_index.insert(summary.index, public_key.clone());
            state.validators.insert(public_key, summary);
        }
        Ok(())
    }

    pub fn get_status(&self, public_key: &BlsPublicKey) -> Result<ValidatorStatus, Error> {
        let state = self.state.lock();
        state
            .validators
            .get(public_key)
            .map(|validator| validator.status)
            .ok_or(Error::UnknownPubkey)
    }

    pub fn get_public_key(&self, index: ValidatorIndex) -> Result<BlsPublicKey, Error> {
        let state = self.state.lock();
        state.pubkeys_by_index.get(&index).cloned().ok_or(Error::UnknownIndex)
    }
}
