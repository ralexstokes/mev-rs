use crate::blinded_block_provider::Error as BlindedBlockProviderError;
use crate::builder::{
    BuildJob, Duty, Error, PayloadId, ProposerPreparation, ProposerSchedule, RpcResponse,
};
use crate::types::{BidRequest as PayloadRequest, ExecutionPayloadWithValue};
use anvil_rpc::{
    request::{Id, RequestParams, RpcMethodCall, Version},
    response::ResponseResult,
};
use ethereum_consensus::{
    bellatrix::mainnet::ExecutionPayload,
    builder::SignedValidatorRegistration,
    clock::convert_timestamp_to_slot,
    crypto::SecretKey,
    primitives::{BlsPublicKey, ExecutionAddress, U256},
};
use reqwest::Client as HttpClient;
use serde::Serialize;
use std::collections::HashMap;
use std::ops::Deref;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use url::Url;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GetPayloadV1Params {
    payload_id: PayloadId,
}

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
    genesis_time: u64,
    seconds_per_slot: u64,
    engine_api_endpoint: Url,
    client: HttpClient,
    state: Mutex<State>,
}

impl Inner {
    pub fn new(genesis_time: u64, seconds_per_slot: u64, engine_api_endpoint: Url) -> Self {
        let key_bytes = [2u8; 32];
        let secret_key = SecretKey::try_from(key_bytes.as_slice()).unwrap();
        let public_key = secret_key.public_key();
        Self {
            _secret_key: secret_key,
            _public_key: public_key,
            genesis_time,
            seconds_per_slot,
            engine_api_endpoint,
            client: HttpClient::new(),
            state: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct State {
    get_payload_rpc_id: i64,
    validator_preferences: HashMap<BlsPublicKey, SignedValidatorRegistration>,
    fee_recipient_to_validator: HashMap<ExecutionAddress, BlsPublicKey>,
    available_payloads: HashMap<PayloadRequest, PayloadId>,
}

fn derive_payload_request(
    build_job: &BuildJob,
    public_key: &BlsPublicKey,
    genesis_time: u64,
    seconds_per_slot: u64,
) -> PayloadRequest {
    let slot = convert_timestamp_to_slot(build_job.timestamp, genesis_time, seconds_per_slot);
    let parent_hash = build_job.head_block_hash.clone();
    PayloadRequest {
        slot,
        parent_hash,
        public_key: public_key.clone(),
    }
}

impl EngineBuilder {
    pub fn new(genesis_time: u64, seconds_per_slot: u64, engine_api_endpoint: Url) -> Self {
        let inner = Inner::new(genesis_time, seconds_per_slot, engine_api_endpoint);
        Self(Arc::new(inner))
    }

    fn process_build_job(&self, build_job: &BuildJob) -> Result<(), Error> {
        let mut state = self.state.lock().expect("can lock");
        let public_key = state
            .fee_recipient_to_validator
            .get(&build_job.suggested_fee_recipient)
            .ok_or_else(|| Error::UnknownFeeRecipient(build_job.suggested_fee_recipient.clone()))?;
        let payload_request = derive_payload_request(
            &build_job,
            public_key,
            self.genesis_time,
            self.seconds_per_slot,
        );
        state
            .available_payloads
            .insert(payload_request, build_job.payload_id.clone());
        Ok(())
    }

    fn process_proposer_schedule(
        &self,
        schedule: &[Duty],
    ) -> Result<Vec<ProposerPreparation>, Error> {
        let state = self.state.lock().expect("can lock");
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
                        Ok(preparations) => {
                            let _ = preparation_tx.send(preparations).map_err(|_| {
                                tracing::warn!("proposer preparation channel closed");
                            });
                        },
                        Err(err) => {
                            tracing::warn!("error processing dispatched build job: {err}");
                        }
                    }
                }
            }
        }
    }

    async fn fetch_payload(&self, payload_id: PayloadId) -> Result<ExecutionPayload, Error> {
        let request_id = {
            let mut state = self.state.lock().expect("can lock");
            let id = state.get_payload_rpc_id;
            state.get_payload_rpc_id += 1;
            id
        };
        let params = serde_json::to_value(GetPayloadV1Params { payload_id }).unwrap();
        let params = params.as_object().unwrap();
        let request = RpcMethodCall {
            jsonrpc: Version::V2,
            method: "engine_getPayloadV1".to_string(),
            params: RequestParams::Object(params.clone()),
            id: Id::Number(request_id),
        };
        let response = self
            .client
            .post(self.engine_api_endpoint.clone())
            .json(&request)
            .send()
            .await?;
        let response = response.json::<RpcResponse>().await?;
        match response.result {
            ResponseResult::Success(payload_json) => {
                let payload: ExecutionPayload = serde_json::from_value(payload_json)?;
                Ok(payload)
            }
            ResponseResult::Error(rpc_error) => {
                tracing::warn!("error with `engine_getPayloadV1` endpoint: {rpc_error}");
                return Err(Error::Rpc(rpc_error.to_string()));
            }
        }
    }

    pub async fn get_payload_with_value(
        &self,
        request: &PayloadRequest,
    ) -> Result<ExecutionPayloadWithValue, Error> {
        let payload_id = {
            let state = self.state.lock().expect("can lock");
            state
                .available_payloads
                .get(request)
                .ok_or_else(|| Error::NoPayloadPrepared(request.clone()))?
                .clone()
        };

        let payload = self.fetch_payload(payload_id).await?;

        // TODO figure out `value` to send

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
