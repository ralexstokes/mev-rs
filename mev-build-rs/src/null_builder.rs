use ethereum_consensus::{
    bellatrix::mainnet as bellatrix,
    builder::SignedValidatorRegistration,
    capella::mainnet as capella,
    primitives::{BlsPublicKey, U256},
    ssz::ByteList,
    state_transition::{Context, Forks},
};
use mev_rs::{
    types::{BidRequest as PayloadRequest, ExecutionPayload},
    BlindedBlockProviderError,
};
use parking_lot::Mutex;
use std::{collections::HashMap, ops::Deref, sync::Arc};

// A `NullBuilder` builds empty blocks. Primarily used for testing.
#[derive(Clone)]
pub struct NullBuilder(Arc<NullBuilderInner>);

impl Deref for NullBuilder {
    type Target = NullBuilderInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct NullBuilderInner {
    context: Arc<Context>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
}

impl NullBuilder {
    pub fn new(context: Arc<Context>) -> Self {
        let inner = NullBuilderInner { context, state: Default::default() };
        Self(Arc::new(inner))
    }

    pub fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        let mut state = self.state.lock();
        for registration in registrations {
            let public_key = registration.message.public_key.clone();
            state.validator_preferences.insert(public_key, registration.clone());
        }
        Ok(())
    }

    pub fn get_payload_with_value(
        &self,
        request: &PayloadRequest,
    ) -> Result<(ExecutionPayload, U256), BlindedBlockProviderError> {
        let (fee_recipient, gas_limit) = self
            .state
            .lock()
            .validator_preferences
            .get(&request.public_key)
            .map(|preferences| {
                (preferences.message.fee_recipient.clone(), preferences.message.gas_limit)
            })
            .ok_or_else(|| {
                BlindedBlockProviderError::MissingPreferences(request.public_key.clone())
            })?;

        let fork = self.context.fork(request.slot);
        let payload = match fork {
            Forks::Bellatrix => ExecutionPayload::Bellatrix(bellatrix::ExecutionPayload {
                parent_hash: request.parent_hash.clone(),
                fee_recipient,
                gas_limit,
                extra_data: ByteList::try_from(b"hello world in bellatrix".as_ref()).unwrap(),
                ..Default::default()
            }),
            Forks::Capella => ExecutionPayload::Capella(capella::ExecutionPayload {
                parent_hash: request.parent_hash.clone(),
                fee_recipient,
                gas_limit,
                extra_data: ByteList::try_from(b"hello world in capella".as_ref()).unwrap(),
                ..Default::default()
            }),
            _ => unimplemented!(),
        };

        Ok((payload, U256::zero()))
    }
}
