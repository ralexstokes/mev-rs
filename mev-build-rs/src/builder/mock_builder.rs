use crate::blinded_block_provider::Error as BlindedBlockProviderError;
use crate::builder::{BuildJob, Duty, Error, ProposerPreparation, ProposerSchedule};
use crate::types::{BidRequest as PayloadRequest, ExecutionPayloadWithValue};
use ethereum_consensus::{
    bellatrix::mainnet::ExecutionPayload,
    builder::SignedValidatorRegistration,
    clock::convert_timestamp_to_slot,
    crypto::SecretKey,
    primitives::{BlsPublicKey, ExecutionAddress, U256},
    ssz::ByteList,
};
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

type PayloadId = u64;

#[derive(Clone)]
pub struct MockBuilder(Arc<Inner>);

impl Deref for MockBuilder {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    _secret_key: SecretKey,
    _public_key: BlsPublicKey,
    genesis_time: u64,
    seconds_per_slot: u64,
    state: Mutex<State>,
}

impl Inner {
    pub fn new(genesis_time: u64, seconds_per_slot: u64) -> Self {
        let key_bytes = [2u8; 32];
        let secret_key = SecretKey::try_from(key_bytes.as_slice()).unwrap();
        let public_key = secret_key.public_key();
        Self {
            _secret_key: secret_key,
            _public_key: public_key,
            genesis_time,
            seconds_per_slot,
            state: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
    fee_recipient_to_validator: HashMap<ExecutionAddress, BlsPublicKey>,
    available_payloads: HashMap<PayloadRequest, PayloadId>,
}

impl MockBuilder {
    pub fn new(genesis_time: u64, seconds_per_slot: u64) -> Self {
        let inner = Inner::new(genesis_time, seconds_per_slot);
        Self(Arc::new(inner))
    }

    fn derive_payload_request(
        &self,
        build_job: &BuildJob,
        public_key: &BlsPublicKey,
    ) -> PayloadRequest {
        let slot = convert_timestamp_to_slot(
            build_job.timestamp,
            self.genesis_time,
            self.seconds_per_slot,
        );
        let parent_hash = build_job.head_block_hash.clone();
        PayloadRequest {
            slot,
            parent_hash,
            public_key: public_key.clone(),
        }
    }

    fn process_build_job(&self, build_job: &BuildJob) -> Result<(), Error> {
        let mut state = self.state.lock().expect("can lock");
        let public_key = state
            .fee_recipient_to_validator
            .get(&build_job.suggested_fee_recipient)
            .ok_or_else(|| Error::UnknownFeeRecipient(build_job.suggested_fee_recipient.clone()))?;
        let payload_request = self.derive_payload_request(&build_job, public_key);
        state
            .available_payloads
            .insert(payload_request, build_job.payload_id);
        Ok(())
    }

    fn process_proposer_schedule(
        &self,
        schedule: &[Duty],
    ) -> Result<Vec<ProposerPreparation>, Error> {
        let mut state = self.state.lock().expect("can lock");
        let mut preparations = vec![];
        for duty in schedule {
            if let Some(registration) = state.validator_preferences.get(&duty.public_key) {
                let preparation = (
                    duty.validator_index,
                    registration.message.fee_recipient.clone(),
                );
                preparations.push(preparation);
            }
        }
        Ok(preparations)
    }

    pub async fn run(
        &self,
        mut build_jobs: mpsc::Receiver<BuildJob>,
        mut proposer_schedules: mpsc::Receiver<ProposerSchedule>,
    ) {
        loop {
            tokio::select! {
                Some(build_job) = build_jobs.recv() => {
                    if let Err(err) = self.process_build_job(&build_job) {
                        tracing::warn!("error processing dispatched build job: {err}");
                    }
                }
                Some((schedule, preparation_tx)) = proposer_schedules.recv() => {
                    match self.process_proposer_schedule(&schedule) {
                        Ok(preparations) => preparation_tx.send(preparations).map_err(|err| {
                            tracing::warn!("proposer preparation channel closed");
                        }),
                        Err(err) => {
                            tracing::warn!("error processing dispatched build job: {err}");
                        }
                    }
                }
            }
        }
    }

    pub fn get_payload_with_value(
        &self,
        request: &PayloadRequest,
    ) -> Result<ExecutionPayloadWithValue, Error> {
        let state = self.state.lock().expect("can lock");

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
                .insert(public_key.clone(), registration.clone());
            state
                .fee_recipient_to_validator
                .insert(registration.message.fee_recipient.clone(), public_key);
        }
        Ok(())
    }
}
