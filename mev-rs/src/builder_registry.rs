use crate::{types::SignedBidSubmission, Error};

use ethereum_consensus::primitives::BlsPublicKey;

use std::collections::HashMap;

#[derive(Default, Debug)]

pub struct _BuilderRegistry {
    _submissions: HashMap<BlsPublicKey, Vec<SignedBidSubmission>>,
}

impl _BuilderRegistry {
    pub fn _store_submission(
        &mut self,
        builder_public_key: &BlsPublicKey,
        submission: &SignedBidSubmission,
    ) -> Result<(), Error> {
        self._submissions.entry(builder_public_key.clone()).and_modify(|e| {
            e.push(submission.clone());
        });
        Ok(())
    }
}
