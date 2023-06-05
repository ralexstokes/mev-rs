use std::sync::Arc;

use anvil_rpc::request::{Id, RequestParams, RpcMethodCall, Version};
use ethereum_consensus::{capella::Withdrawal, primitives::ValidatorIndex};
use parking_lot::Mutex;
use serde::Deserialize;
use ssz_rs::prelude::U256;

use crate::{
    engine_api_proxy::{
        types::{self, BuildVersion, ExecutionPayloadWithValue, PayloadId},
        Error,
    },
    types::{bellatrix, capella, ExecutionPayload},
};

const ENGINE_GET_PAYLOADV1_METHOD: &str = "engine_getPayloadV1";
const ENGINE_GET_PAYLOADV2_METHOD: &str = "engine_getPayloadV2";

#[derive(Clone)]
pub struct Client {
    client: reqwest::Client,
    endpoint: String,
    rpc_id: Arc<Mutex<i64>>,
}

impl Client {
    pub fn new(endpoint: &str) -> Self {
        let client = reqwest::Client::new();
        Self { client, endpoint: endpoint.to_string(), rpc_id: Arc::new(Mutex::new(0)) }
    }

    pub async fn get_payload_with_value(
        &self,
        payload_id: &PayloadId,
        auth_token: &str,
        version: BuildVersion,
    ) -> Result<(ExecutionPayload, U256), Error> {
        let params = serde_json::to_value(payload_id)?;
        let rpc_id = { *self.rpc_id.lock() };
        let method = match version {
            BuildVersion::V1 => ENGINE_GET_PAYLOADV1_METHOD,
            BuildVersion::V2 => ENGINE_GET_PAYLOADV2_METHOD,
        };
        let call = RpcMethodCall {
            jsonrpc: Version::V2,
            method: method.to_string(),
            params: RequestParams::Array(vec![params]),
            id: Id::Number(rpc_id),
        };
        let response = self
            .client
            .post(&self.endpoint)
            .header("Authorization", auth_token)
            .json(&call)
            .send()
            .await?;
        {
            let mut rpc_id = self.rpc_id.lock();
            *rpc_id += 1;
        }
        let response: serde_json::Value = response.json().await?;
        let result = response.get("result").ok_or_else(|| Error::UnexpectedResponse)?;
        match version {
            BuildVersion::V1 => {
                let payload = types::ExecutionPayloadV1::deserialize(result).unwrap();
                let payload = ExecutionPayload::Bellatrix(bellatrix::ExecutionPayload {
                    parent_hash: payload.parent_hash,
                    fee_recipient: payload.fee_recipient,
                    state_root: payload.state_root,
                    receipts_root: payload.receipts_root,
                    logs_bloom: payload.logs_bloom,
                    prev_randao: payload.prev_randao,
                    block_number: payload.block_number,
                    gas_limit: payload.gas_limit,
                    gas_used: payload.gas_used,
                    timestamp: payload.timestamp,
                    extra_data: payload.extra_data,
                    base_fee_per_gas: payload.base_fee_per_gas,
                    block_hash: payload.block_hash,
                    transactions: payload.transactions,
                });
                // TODO try to get accurate value?
                let value: U256 = 1_000_000_123.into();
                Ok((payload, value))
            }
            BuildVersion::V2 => {
                let payload_with_value = ExecutionPayloadWithValue::deserialize(result).unwrap();
                let payload = match payload_with_value.execution_payload {
                    types::ExecutionPayload::V1(payload) => {
                        ExecutionPayload::Bellatrix(bellatrix::ExecutionPayload {
                            parent_hash: payload.parent_hash,
                            fee_recipient: payload.fee_recipient,
                            state_root: payload.state_root,
                            receipts_root: payload.receipts_root,
                            logs_bloom: payload.logs_bloom,
                            prev_randao: payload.prev_randao,
                            block_number: payload.block_number,
                            gas_limit: payload.gas_limit,
                            gas_used: payload.gas_used,
                            timestamp: payload.timestamp,
                            extra_data: payload.extra_data,
                            base_fee_per_gas: payload.base_fee_per_gas,
                            block_hash: payload.block_hash,
                            transactions: payload.transactions,
                        })
                    }
                    types::ExecutionPayload::V2(payload) => {
                        ExecutionPayload::Capella(capella::ExecutionPayload {
                            parent_hash: payload.parent_hash,
                            fee_recipient: payload.fee_recipient,
                            state_root: payload.state_root,
                            receipts_root: payload.receipts_root,
                            logs_bloom: payload.logs_bloom,
                            prev_randao: payload.prev_randao,
                            block_number: payload.block_number,
                            gas_limit: payload.gas_limit,
                            gas_used: payload.gas_used,
                            timestamp: payload.timestamp,
                            extra_data: payload.extra_data,
                            base_fee_per_gas: payload.base_fee_per_gas,
                            block_hash: payload.block_hash,
                            transactions: payload.transactions,
                            withdrawals: payload
                                .withdrawals
                                .into_iter()
                                .map(|w| Withdrawal {
                                    index: w.index as usize,
                                    validator_index: w.validator_index as ValidatorIndex,
                                    address: w.address,
                                    amount: w.amount,
                                })
                                .collect::<Vec<_>>()
                                .try_into()
                                // TODO error handling here...
                                .unwrap(),
                        })
                    }
                };
                let value = payload_with_value.block_value;
                Ok((payload, value))
            }
        }
    }
}
