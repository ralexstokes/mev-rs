use crate::relay_mux::RelayMux;
use crate::types::{ProposalRequest, SignedBlindedBeaconBlock, ValidatorRegistrationV1};
use axum::routing::post;
use axum::{extract::Extension, Router};
use axum_json_rpc::error::JsonRpcErrorReason;
use axum_json_rpc::{JsonRpcExtractor, JsonRpcResponse, JsonRpcResult};
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;

pub enum Error {
    Generic(String),
    UnknownHash,
    UnknownValidator,
    UnknownFeeRecipient,
    UnknownBlock,
    InvalidSignature,
    InvalidTimestamp,
}

impl Error {
    fn code(&self) -> i32 {
        let offset = match self {
            Self::Generic(_) => 0,
            Self::UnknownHash => 1,
            Self::UnknownValidator => 2,
            Self::UnknownFeeRecipient => 3,
            Self::UnknownBlock => 4,
            Self::InvalidSignature => 5,
            Self::InvalidTimestamp => 6,
        };
        -32000 - offset
    }
}

impl From<Error> for JsonRpcErrorReason {
    fn from(err: Error) -> Self {
        Self::ServerError(err.code())
    }
}

fn serve_status(request_id: i64) -> JsonRpcResult {
    tracing::info!("called `builder_status`");
    Ok(JsonRpcResponse::success(request_id, "OK"))
}

async fn serve_validator_registration(
    request_id: i64,
    relay_mux: Arc<RelayMux>,
    registration: &ValidatorRegistrationV1,
) -> JsonRpcResult {
    tracing::info!("called `builder_registerValidatorV1` with {registration:?}");
    if let Err(errs) = relay_mux.register_validator(registration).await {
        for err in errs {
            tracing::error!("{err:?}");
        }
        // TODO: return err to caller
    }

    // TODO: remove
    let result = registration.a + 1;
    Ok(JsonRpcResponse::success(request_id, result))
}

async fn serve_header(
    request_id: i64,
    relay_mux: Arc<RelayMux>,
    proposal_request: &ProposalRequest,
) -> JsonRpcResult {
    tracing::info!("called `builder_getHeaderV1` with {proposal_request:?}");

    let _ = relay_mux.fetch_best_header(proposal_request).await;
    // TODO: return header to caller

    // TODO: remove
    let result = proposal_request.a + 1;
    Ok(JsonRpcResponse::success(request_id, result))
}

async fn serve_payload(
    request_id: i64,
    relay_mux: Arc<RelayMux>,
    signed_block: &SignedBlindedBeaconBlock,
) -> JsonRpcResult {
    tracing::info!("called `builder_getPayloadV1` with {signed_block:?}");

    let _ = relay_mux.post_block(signed_block).await;
    // TODO: return payload to caller

    // TODO: remove
    let result = signed_block.a + 1;
    Ok(JsonRpcResponse::success(request_id, result))
}

async fn serve_builder_api(
    request: JsonRpcExtractor,
    Extension(relay_mux): Extension<Arc<RelayMux>>,
) -> JsonRpcResult {
    let request_id = request.get_request_id();
    match request.method() {
        "builder_status" => serve_status(request_id),
        "builder_registerValidatorV1" => {
            let params: ValidatorRegistrationV1 = request.parse_params()?;
            serve_validator_registration(request_id, relay_mux, &params).await
        }
        "builder_getHeaderV1" => {
            let params: ProposalRequest = request.parse_params()?;
            serve_header(request_id, relay_mux, &params).await
        }
        "builder_getPayloadV1" => {
            let params: SignedBlindedBeaconBlock = request.parse_params()?;
            serve_payload(request_id, relay_mux, &params).await
        }
        method => Ok(request.method_not_found(method)),
    }
}

pub struct Server {
    host: Ipv4Addr,
    port: u16,
    relay_mux: Arc<RelayMux>,
}

impl Server {
    pub fn new(host: Ipv4Addr, port: u16, relay_mux: RelayMux) -> Self {
        Self {
            host,
            port,
            relay_mux: Arc::new(relay_mux),
        }
    }

    pub async fn run(&mut self) {
        let router = Router::new()
            .route("/", post(serve_builder_api))
            .layer(Extension(self.relay_mux.clone()));
        let addr = SocketAddr::from((self.host, self.port));
        let json_rpc_server = axum::Server::bind(&addr).serve(router.into_make_service());

        tracing::debug!("listening...");
        if let Err(err) = json_rpc_server.await {
            tracing::error!("error while listening for incoming: {err}")
        }
    }
}
