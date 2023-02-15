use crate::engine_api_proxy::types::{
    BuildJob, BuildVersion, ForkchoiceUpdatedV1Params, ForkchoiceUpdatedV1Response,
    ForkchoiceUpdatedV2Params, PayloadAttributes,
};
use axum::{
    extract::State,
    http::{uri::Uri, Request, Response},
    routing::{post, IntoMakeService},
    Router,
};
use hyper::{body, client::HttpConnector, server::conn::AddrIncoming, Body};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tokio::{sync::mpsc, task::JoinHandle};

pub type EngineApiProxyServer = axum::Server<AddrIncoming, IntoMakeService<Router>>;
pub type Client = hyper::client::Client<HttpConnector, Body>;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub engine_api_endpoint: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::LOCALHOST,
            port: 8551,
            engine_api_endpoint: "http://127.0.0.1:8552".into(),
        }
    }
}

pub struct Server {
    config: Config,
}

impl Server {
    pub fn new(config: Config) -> Self {
        Self { config }
    }

    pub fn serve(&self, proxy: Arc<Proxy>) -> EngineApiProxyServer {
        let app = Router::new().route("/", post(handler)).with_state(proxy);

        let addr = SocketAddr::from((self.config.host, self.config.port));
        axum::Server::bind(&addr).serve(app.into_make_service())
    }

    pub fn spawn(self, proxy: Arc<Proxy>) -> JoinHandle<()> {
        let server = self.serve(proxy);
        let address = server.local_addr();
        tokio::spawn(async move {
            tracing::info!("listening at {address}...");
            if let Err(err) = server.await {
                tracing::error!("error while listening for incoming: {err}")
            }
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcRequest {
    method: String,
    params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonRpcResponse {
    result: serde_json::Value,
}

async fn handler(State(proxy): State<Arc<Proxy>>, req: Request<Body>) -> Response<Body> {
    proxy.process_message(req).await
}

pub struct Proxy {
    client: Client,
    target_endpoint: String,
    build_jobs: mpsc::Sender<BuildJob>,
    // TODO: this is a kludge, remove w/ proper token generation
    pub token: Mutex<String>,
}

impl Proxy {
    pub fn new(client: Client, target_endpoint: &str, build_jobs: mpsc::Sender<BuildJob>) -> Self {
        Self {
            client,
            target_endpoint: target_endpoint.to_string(),
            build_jobs,
            token: Default::default(),
        }
    }

    async fn process_message(&self, req: Request<Body>) -> Response<Body> {
        let (parts, body) = req.into_parts();
        let token = parts.headers.get("Authorization").unwrap();
        {
            let mut state = self.token.lock();
            *state = String::from(token.to_str().unwrap());
        }
        let body_bytes = body::to_bytes(body).await.unwrap();

        let request_rpc: JsonRpcRequest = serde_json::from_slice(&body_bytes).unwrap();

        let body = Body::from(body_bytes);
        let mut req = Request::from_parts(parts, body);

        *req.uri_mut() = Uri::try_from(&self.target_endpoint).unwrap();
        let response = self.client.request(req).await.unwrap();
        if request_rpc.method.contains("engine_forkchoiceUpdatedV") {
            let (parts, body) = response.into_parts();

            let body_bytes = body::to_bytes(body).await.unwrap();
            let response_rpc: JsonRpcResponse = serde_json::from_slice(&body_bytes).unwrap();
            if request_rpc.method.ends_with("V1") {
                self.process_forkchoice_updated_call_v1(&request_rpc, &response_rpc).await;
            } else {
                // V2
                self.process_forkchoice_updated_call_v2(&request_rpc, &response_rpc).await;
            }

            let body = Body::from(body_bytes);

            Response::from_parts(parts, body)
        } else {
            response
        }
    }

    async fn process_forkchoice_updated_call_v1(
        &self,
        request: &JsonRpcRequest,
        response: &JsonRpcResponse,
    ) {
        let result = ForkchoiceUpdatedV1Response::deserialize(&response.result).unwrap();
        if let Some(payload_id) = result.payload_id {
            let params = ForkchoiceUpdatedV1Params::deserialize(&request.params).unwrap();
            if let Some(payload_attributes) = params.payload_attributes {
                let head_block_hash = params.forkchoice_state.head_block_hash;
                let timestamp = payload_attributes.timestamp;
                let suggested_fee_recipient = payload_attributes.suggested_fee_recipient;
                let job = BuildJob {
                    head_block_hash,
                    timestamp,
                    suggested_fee_recipient,
                    payload_id,
                    version: BuildVersion::V1,
                };
                if let Err(job) = self.build_jobs.send(job).await {
                    tracing::warn!("could not send build job to builder: {job}");
                }
            }
        }
    }

    async fn process_forkchoice_updated_call_v2(
        &self,
        request: &JsonRpcRequest,
        response: &JsonRpcResponse,
    ) {
        let result = ForkchoiceUpdatedV1Response::deserialize(&response.result).unwrap();
        if let Some(payload_id) = result.payload_id {
            let params = ForkchoiceUpdatedV2Params::deserialize(&request.params).unwrap();
            if let Some(payload_attributes) = params.payload_attributes {
                match payload_attributes {
                    PayloadAttributes::V1(payload_attributes) => {
                        let head_block_hash = params.forkchoice_state.head_block_hash;
                        let timestamp = payload_attributes.timestamp;
                        let suggested_fee_recipient = payload_attributes.suggested_fee_recipient;
                        let job = BuildJob {
                            head_block_hash,
                            timestamp,
                            suggested_fee_recipient,
                            payload_id,
                            version: BuildVersion::V1,
                        };
                        if let Err(job) = self.build_jobs.send(job).await {
                            tracing::warn!("could not send build job to builder: {job}");
                        }
                    }
                    PayloadAttributes::V2(payload_attributes) => {
                        let head_block_hash = params.forkchoice_state.head_block_hash;
                        let timestamp = payload_attributes.timestamp;
                        let suggested_fee_recipient = payload_attributes.suggested_fee_recipient;
                        let job = BuildJob {
                            head_block_hash,
                            timestamp,
                            suggested_fee_recipient,
                            payload_id,
                            version: BuildVersion::V2,
                        };
                        if let Err(job) = self.build_jobs.send(job).await {
                            tracing::warn!("could not send build job to builder: {job}");
                        }
                    }
                }
            }
        }
    }
}
