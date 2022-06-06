use crate::blinded_block_provider::Error as BlindedBlockProviderError;
use crate::builder::{BuildJob, Error, ProposerSchedule};
use crate::types::{BidRequest as PayloadRequest, ExecutionPayloadWithValue};
use ethereum_consensus::{
    bellatrix::mainnet::ExecutionPayload,
    builder::SignedValidatorRegistration,
    crypto::SecretKey,
    primitives::{BlsPublicKey, U256},
    ssz::ByteList,
};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct EngineBuilder(Arc<Inner>);

impl Deref for EngineBuilder {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    _secret_key: SecretKey,
    _public_key: BlsPublicKey,
    build_jobs: mpsc::Receiver<BuildJob>,
    proposer_schedules: mpsc::Receiver<ProposerSchedule>,
    state: Mutex<State>,
}

impl Inner {
    pub fn new(
        build_jobs: mpsc::Receiver<BuildJob>,
        proposer_schedules: mpsc::Receiver<ProposerSchedule>,
    ) -> Self {
        let key_bytes = [2u8; 32];
        let secret_key = SecretKey::try_from(key_bytes.as_slice()).unwrap();
        let public_key = secret_key.public_key();
        Self {
            _secret_key: secret_key,
            _public_key: public_key,
            build_jobs,
            proposer_schedules,
            state: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
}

impl EngineBuilder {
    pub fn new(
        build_jobs: mpsc::Receiver<BuildJob>,
        proposer_schedules: mpsc::Receiver<ProposerSchedule>,
    ) -> Self {
        let inner = Inner::new(build_jobs, proposer_schedules);
        Self(Arc::new(inner))
    }

    pub async fn run(&self) {
        // for each new schedule
        //   if proposers in local registry
        //   then dispatch to scheduler

        // stream jobs from build_jobs:
        // for job in build_job:
        //   -- assemble to payload request, save
        // save mapping `PayloadRequest` => `payloadId`
    }

    pub fn get_payload_with_value(
        &self,
        request: &PayloadRequest,
    ) -> Result<ExecutionPayloadWithValue, Error> {
        let state = self.state.lock().expect("can lock");
        // map `PayloadRequest` -> `payloadID`
        // call `engine_getPayloadV1` with `payloadId` => `ExecutionPayload`
        // send `ExecutionPayload` to relay

        let preferences = state
            .validator_preferences
            .get(&request.public_key)
            .ok_or_else(|| Error::MissingPreferences(request.public_key.clone()))?;

        let fee_recipient = preferences.message.fee_recipient.clone();
        let gas_limit = preferences.message.gas_limit;

        let payload = ExecutionPayload {
            parent_hash: request.parent_hash.clone(),
            fee_recipient,
            gas_limit,
            extra_data: ByteList::try_from(b"hello world".as_ref()).unwrap(),
            ..Default::default()
        };

        let bid = ExecutionPayloadWithValue {
            payload,
            value: U256::from_bytes_le([1u8; 32]),
        };
        Ok(bid)
    }

    pub fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), BlindedBlockProviderError> {
        // TODO this assumes registrations have already been validated by relay
        // will eventually remove this assumption
        let mut state = self.state.lock().expect("can lock");
        for registration in registrations {
            let public_key = registration.message.public_key.clone();
            state
                .validator_preferences
                .insert(public_key, registration.clone());
        }
        Ok(())
    }
}
