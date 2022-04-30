use crate::relay_mux::RelayMux;
use crate::types::{ProposalRequest, SignedBlindedBeaconBlock, ValidatorRegistrationV1};
use axum::routing::post;
use axum::{extract::Extension, Router};
use axum_json_rpc::error::JsonRpcErrorReason;
use axum_json_rpc::{JsonRpcExtractor, JsonRpcResponse, JsonRpcResult};
use std::net::{Ipv4Addr, SocketAddr};

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

fn handle_status(request_id: i64) -> JsonRpcResult {
    tracing::debug!("called `builder_status`");
    Ok(JsonRpcResponse::success(request_id, "OK"))
}

async fn validate_registration(registration: &ValidatorRegistrationV1) -> Result<(), Error> {
    // TODO: validations
    Ok(())
}

async fn handle_validator_registration(
    request_id: i64,
    relay_mux: RelayMux,
    registration: &ValidatorRegistrationV1,
) -> JsonRpcResult {
    tracing::debug!("called `builder_registerValidatorV1` with {registration:?}");

    validate_registration(registration).await;
    // TODO return err

    relay_mux.register_validator(registration).await;

    // TODO: return any err to caller

    // TODO: remove
    let result = registration.a + 1;
    Ok(JsonRpcResponse::success(request_id, result))
}

async fn handle_fetch_bid(
    request_id: i64,
    relay_mux: RelayMux,
    proposal_request: &ProposalRequest,
) -> JsonRpcResult {
    tracing::info!("called `builder_getHeaderV1` with {proposal_request:?}");

    let best_bid = relay_mux.fetch_best_bid(proposal_request).await.unwrap();
    tracing::error!("{best_bid:?}");
    // TODO: handle error

    Ok(JsonRpcResponse::success(request_id, best_bid))
}

async fn handle_accept_bid(
    request_id: i64,
    relay_mux: RelayMux,
    signed_block: &SignedBlindedBeaconBlock,
) -> JsonRpcResult {
    tracing::info!("called `builder_getPayloadV1` with {signed_block:?}");

    let _ = relay_mux.accept_bid(signed_block).await;
    // TODO: return payload to caller

    // TODO: remove
    let result = signed_block.a + 1;
    Ok(JsonRpcResponse::success(request_id, result))
}

async fn handle_builder_api(
    request: JsonRpcExtractor,
    Extension(relay_mux): Extension<RelayMux>,
) -> JsonRpcResult {
    let request_id = request.get_request_id();
    match request.method() {
        "builder_status" => handle_status(request_id),
        "builder_registerValidatorV1" => {
            let params: ValidatorRegistrationV1 = request.parse_params()?;
            handle_validator_registration(request_id, relay_mux, &params).await
        }
        "builder_getHeaderV1" => {
            let params: ProposalRequest = request.parse_params()?;
            handle_fetch_bid(request_id, relay_mux, &params).await
        }
        "builder_getPayloadV1" => {
            let params: SignedBlindedBeaconBlock = request.parse_params()?;
            handle_accept_bid(request_id, relay_mux, &params).await
        }
        method => Ok(request.method_not_found(method)),
    }
}

pub struct Server {
    host: Ipv4Addr,
    port: u16,
}

impl Server {
    pub fn new(host: Ipv4Addr, port: u16) -> Self {
        Self { host, port }
    }

    pub async fn run(&mut self, relay_mux: RelayMux) {
        let router = Router::new()
            .route("/", post(handle_builder_api))
            .layer(Extension(relay_mux));
        let addr = SocketAddr::from((self.host, self.port));
        let json_rpc_handler = axum::Server::bind(&addr).serve(router.into_make_service());

        tracing::debug!("listening...");
        if let Err(err) = json_rpc_handler.await {
            tracing::error!("error while listening for incoming: {err}")
        }
    }
}
