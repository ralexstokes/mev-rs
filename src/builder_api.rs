use crate::relay_mux::RelayMux;
use crate::types::{ProposalRequest, SignedBlindedBeaconBlock, ValidatorRegistrationV1};
use axum::routing::post;
use axum::{extract::Extension, Router};
use axum_json_rpc::error::{JsonRpcError, JsonRpcErrorReason};
use axum_json_rpc::{JsonRpcExtractor, JsonRpcResponse, JsonRpcResult};
use serde_json::Value;
use std::net::{Ipv4Addr, SocketAddr};
use thiserror::Error;

const JSON_RPC_RESPONSE_SUCCESS: &str = "OK";

#[derive(Debug, Error)]
pub enum Error {
    #[error("server error: {0}")]
    Generic(String),
    #[error("unknown parent hash in proposal request")]
    UnknownHash,
    #[error("unknown validator with pubkey in proposal request")]
    UnknownValidator,
    #[error("unknown fee recipient for proposer given in proposal request")]
    UnknownFeeRecipient,
    #[error("block does not match the provided header")]
    UnknownBlock,
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid timestamp")]
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
    Ok(JsonRpcResponse::success(
        request_id,
        JSON_RPC_RESPONSE_SUCCESS,
    ))
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

    if let Err(err) = validate_registration(registration).await {
        tracing::error!("{err:?}");
        let err_msg = err.to_string();
        let rpc_error = JsonRpcError::new(err.into(), err_msg, Value::Null);
        return Ok(JsonRpcResponse::error(request_id, rpc_error));
    }

    let responses = relay_mux.register_validator(registration).await;
    let mut errors = responses
        .into_iter()
        .filter(|result| result.is_err())
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(JsonRpcResponse::success(
            request_id,
            JSON_RPC_RESPONSE_SUCCESS,
        ))
    } else {
        // TODO: how to send multiple errors?
        let error = errors.swap_remove(0).err().unwrap();
        let err_msg = error.to_string();
        let error = Error::Generic(err_msg.clone());
        let rpc_error = JsonRpcError::new(error.into(), err_msg, Value::Null);
        Ok(JsonRpcResponse::error(request_id, rpc_error))
    }
}

async fn handle_fetch_bid(
    request_id: i64,
    relay_mux: RelayMux,
    proposal_request: &ProposalRequest,
) -> JsonRpcResult {
    tracing::debug!("called `builder_getHeaderV1` with {proposal_request:?}");

    // TODO: any validations?

    match relay_mux.fetch_best_bid(proposal_request).await {
        Ok(bid) => Ok(JsonRpcResponse::success(request_id, bid)),
        Err(err) => {
            tracing::error!("{err:?}");
            let err_msg = err.to_string();
            let err = Error::Generic(err_msg.clone());
            let rpc_error = JsonRpcError::new(err.into(), err_msg, Value::Null);
            return Ok(JsonRpcResponse::error(request_id, rpc_error));
        }
    }
}

async fn handle_accept_bid(
    request_id: i64,
    relay_mux: RelayMux,
    signed_block: &SignedBlindedBeaconBlock,
) -> JsonRpcResult {
    tracing::debug!("called `builder_getPayloadV1` with {signed_block:?}");

    // TODO: any validations?

    match relay_mux.accept_bid(signed_block).await {
        Ok(execution_payload) => Ok(JsonRpcResponse::success(request_id, execution_payload)),
        Err(err) => {
            tracing::error!("{err:?}");
            let err_msg = err.to_string();
            let err = Error::Generic(err_msg.clone());
            let rpc_error = JsonRpcError::new(err.into(), err_msg, Value::Null);
            return Ok(JsonRpcResponse::error(request_id, rpc_error));
        }
    }
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
