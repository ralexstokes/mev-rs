use crate::{types::SignedBidSubmission, Error};
use beacon_api_client::mainnet::Client;
use ethereum_consensus::primitives::BlsPublicKey;
use parking_lot::Mutex;
use std::collections::HashMap;

#[derive(Default, Debug)]
pub struct State {
    _builders: HashMap<BlsPublicKey, SignedBidSubmission>,
}

pub struct _BuilderRegistry {
    client: Client,
    state: Mutex<State>,
}

impl _BuilderRegistry {
    pub fn _store_submission(
        &self,
        builder_public_key: &BlsPublicKey,
        submission: &SignedBidSubmission,
    ) -> Result<(), Error> {
        let mut state = self.state.lock();
        state._builders.insert(builder_public_key.clone(), submission.clone());
        Ok(())
    }
}
