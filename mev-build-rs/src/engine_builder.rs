use ethereum_consensus::{
    bellatrix::mainnet as spec,
    builder::SignedValidatorRegistration,
    crypto::SecretKey,
    primitives::{BlsPublicKey, U256},
    ssz::ByteList,
    state_transition::Context,
};
use mev_rs::{
    blinded_block_provider::Error as BlindedBlockProviderError,
    types::{BidRequest as PayloadRequest, ExecutionPayload},
};
use parking_lot::Mutex;
use std::{collections::HashMap, ops::Deref, sync::Arc};

#[derive(Clone)]
pub struct EngineBuilder(Arc<EngineBuilderInner>);

impl Deref for EngineBuilder {
    type Target = EngineBuilderInner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct EngineBuilderInner {
    _secret_key: SecretKey,
    _public_key: BlsPublicKey,
    _context: Arc<Context>,
    state: Mutex<State>,
}

impl EngineBuilderInner {
    pub fn new(context: Arc<Context>) -> Self {
        let key_bytes = [2u8; 32];
        let secret_key = SecretKey::try_from(key_bytes.as_slice()).unwrap();
        let public_key = secret_key.public_key();
        Self {
            _secret_key: secret_key,
            _public_key: public_key,
            _context: context,
            state: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
}

impl EngineBuilder {
    pub fn new(context: Arc<Context>) -> Self {
        let inner = EngineBuilderInner::new(context);
        Self(Arc::new(inner))
    }

    pub async fn run(&mut self) {}

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

        let payload = ExecutionPayload::Bellatrix(spec::ExecutionPayload {
            parent_hash: request.parent_hash.clone(),
            fee_recipient,
            gas_limit,
            extra_data: ByteList::try_from(b"hello world".as_ref()).unwrap(),
            ..Default::default()
        });

        Ok((payload, U256::from_bytes_le([1u8; 32])))
    }

    pub fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        // TODO this assumes registrations have already been validated by relay
        // will eventually remove this assumption
        let mut state = self.state.lock();
        for registration in registrations {
            let public_key = registration.message.public_key.clone();
            state.validator_preferences.insert(public_key, registration.clone());
        }
        Ok(())
    }
}
