use anvil_rpc::{
    error::RpcError,
    request::{Id, RpcMethodCall, Version},
    response::{ResponseResult, RpcResponse as AnvilRpcResponse},
};
use anvil_server::{serve_http, RpcHandler, ServerConfig};
use ethereum_consensus::{
    primitives::{ExecutionAddress, Hash32},
    ssz::ByteVector,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;
use tokio::sync::mpsc;
use url::Url;

const FORKCHOICE_UPDATED_METHOD: &str = "engine_forkchoiceUpdatedV1";

pub type PayloadId = ByteVector<8>;

pub struct BuildJob {
    pub head_block_hash: Hash32,
    pub timestamp: u64,
    pub suggested_fee_recipient: ExecutionAddress,
    pub payload_id: PayloadId,
}

pub struct EngineProxy {
    proxy_endpoint: Url,
    engine_api_endpoint: Url,
}

#[derive(Clone)]
pub struct ProxyHandler {
    api: Client,
    target_endpoint: Url,
    build_jobs: mpsc::Sender<BuildJob>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkchoiceStateV1 {
    head_block_hash: Hash32,
    safe_block_hash: Hash32,
    finalized_block_hash: Hash32,
}

#[allow(dead_code)]
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayloadAttributesV1 {
    timestamp: u64,
    prev_randao: Hash32,
    suggested_fee_recipient: ExecutionAddress,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkchoiceUpdatedV1Params {
    forkchoice_state: ForkchoiceStateV1,
    payload_attributes: Option<PayloadAttributesV1>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PayloadStatus {
    Valid,
    Invalid,
    Syncing,
    Accepted,
    InvalidBlockHash,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PayloadStatusV1 {
    status: PayloadStatus,
    latest_valid_hash: Option<Hash32>,
    validation_error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ForkchoiceUpdatedV1Response {
    payload_status: PayloadStatusV1,
    payload_id: Option<PayloadId>,
}

// copied from `anvil::rpc::response` so we can access inner fields
#[derive(Debug, PartialEq, Eq, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcResponse {
    // JSON RPC version
    pub jsonrpc: Version,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    #[serde(flatten)]
    pub result: ResponseResult,
}

// copied from `anvil::rpc::response` so we can access inner fields
impl From<RpcError> for RpcResponse {
    fn from(e: RpcError) -> Self {
        Self {
            jsonrpc: Version::V2,
            id: None,
            result: ResponseResult::Error(e),
        }
    }
}

impl ProxyHandler {
    pub fn new(target_endpoint: &Url, build_jobs: mpsc::Sender<BuildJob>) -> Self {
        let api = Client::new();
        Self {
            api,
            target_endpoint: target_endpoint.clone(),
            build_jobs,
        }
    }

    async fn process_fork_choice_updated(
        &self,
        request: &RpcMethodCall,
        response: ForkchoiceUpdatedV1Response,
    ) {
        // TODO can walk the `request` to skip the clone here...
        let params: ForkchoiceUpdatedV1Params =
            match serde_json::from_value(request.params.clone().into()) {
                Ok(params) => params,
                Err(err) => {
                    tracing::warn!("error deserializing forkchoice updated params: {err}");
                    return;
                }
            };

        if let Some(payload_attributes) = params.payload_attributes {
            let head_block_hash = params.forkchoice_state.head_block_hash.clone();
            let timestamp = payload_attributes.timestamp;
            let suggested_fee_recipient = payload_attributes.suggested_fee_recipient.clone();

            if let Some(payload_id) = &response.payload_id {
                let job = BuildJob {
                    head_block_hash,
                    timestamp,
                    suggested_fee_recipient,
                    payload_id: payload_id.clone(),
                };

                if let Err(job) = self.build_jobs.send(job).await {
                    tracing::warn!("could not send build job to builder: {job}");
                }
            }
        }
    }

    async fn proxy(&self, request: &RpcMethodCall) -> RpcResponse {
        let response = match self
            .api
            .post(self.target_endpoint.clone())
            .json(request)
            .send()
            .await
        {
            Ok(response) => response,
            Err(err) => {
                tracing::warn!("error proxying engine API call: {err}");
                return RpcError::internal_error().into();
            }
        };
        match response.json().await {
            Ok(result) => result,
            Err(err) => {
                tracing::warn!("error proxying engine API call: {err}");
                RpcError::parse_error().into()
            }
        }
    }
}

#[async_trait::async_trait]
impl RpcHandler for ProxyHandler {
    type Request = RpcMethodCall;

    async fn on_request(&self, request: Self::Request) -> ResponseResult {
        let response = self.proxy(&request).await;
        if let ResponseResult::Success(result) = &response.result {
            if request.method == FORKCHOICE_UPDATED_METHOD {
                let result: ForkchoiceUpdatedV1Response =
                    serde_json::from_value(result.clone()).unwrap();
                self.process_fork_choice_updated(&request, result).await;
            }
        }
        response.result
    }

    async fn on_call(&self, call: RpcMethodCall) -> AnvilRpcResponse {
        tracing::trace!(target: "rpc", "received handler request {:?}", call);
        let id = call.id();
        let result = self.on_request(call).await;
        tracing::trace!(target: "rpc", "prepared rpc result {:?}", result);
        AnvilRpcResponse::new(id, result)
    }
}

impl EngineProxy {
    pub fn new(proxy_endpoint: Url, engine_api_endpoint: Url) -> Self {
        Self {
            proxy_endpoint,
            engine_api_endpoint,
        }
    }

    pub async fn run(&self, build_jobs: mpsc::Sender<BuildJob>) {
        let config = ServerConfig::default();
        let host: Ipv4Addr = self.proxy_endpoint.host_str().unwrap().parse().unwrap();
        let port = self.proxy_endpoint.port().unwrap();

        let handler = ProxyHandler::new(&self.engine_api_endpoint, build_jobs);

        let server = serve_http((host, port).into(), config, handler);
        if let Err(err) = server.await {
            tracing::warn!("engine proxy server returned early: {err}")
        }
    }
}
