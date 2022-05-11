use crate::relay_mux::{Error as RelayMuxError, RelayMux};
use crate::types::{
    BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use axum::routing::{get, post};
use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    Router,
};
use beacon_api_client::{ApiError, ConsensusVersion, VersionedValue};
use std::net::{Ipv4Addr, SocketAddr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
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
    #[error("issue with relay mux: {0}")]
    Relay(#[from] RelayMuxError),
    #[error("internal server error")]
    Internal,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match self {
            Self::UnknownHash => StatusCode::BAD_REQUEST,
            Self::UnknownValidator => StatusCode::BAD_REQUEST,
            Self::UnknownFeeRecipient => StatusCode::BAD_REQUEST,
            Self::UnknownBlock => StatusCode::BAD_REQUEST,
            Self::InvalidSignature => StatusCode::BAD_REQUEST,
            Self::InvalidTimestamp => StatusCode::BAD_REQUEST,
            Self::Relay(_) => StatusCode::BAD_REQUEST,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (
            status,
            Json(ApiError {
                code: status.as_u16(),
                message: self.to_string(),
            }),
        )
            .into_response()
    }
}

async fn validate_registration(_registration: &SignedValidatorRegistration) -> Result<(), Error> {
    // TODO validations
    Ok(())
}

async fn validate_bid_request(_bid_request: &BidRequest) -> Result<(), Error> {
    // TODO validations
    Ok(())
}

async fn validate_bid(_bid: &SignedBuilderBid) -> Result<(), Error> {
    // TODO validations
    Ok(())
}

async fn validate_signed_block(_signed_block: &SignedBlindedBeaconBlock) -> Result<(), Error> {
    // TODO validations
    Ok(())
}

async fn validate_execution_payload(_execution_payload: &ExecutionPayload) -> Result<(), Error> {
    // TODO validations
    Ok(())
}

async fn handle_status_check() -> impl IntoResponse {
    tracing::debug!("status check");
    StatusCode::OK
}

async fn handle_validator_registration(
    Json(registration): Json<SignedValidatorRegistration>,
    Extension(relay_mux): Extension<RelayMux>,
) -> Result<(), Error> {
    tracing::debug!("processing registration {registration:?}");

    validate_registration(&registration).await?;

    relay_mux.register_validator(&registration).await?;

    Ok(())
}

async fn handle_fetch_bid(
    Path(bid_request): Path<BidRequest>,
    Extension(relay_mux): Extension<RelayMux>,
) -> Result<Json<VersionedValue<SignedBuilderBid>>, Error> {
    tracing::debug!("fetching best bid for block for request {bid_request:?}");

    validate_bid_request(&bid_request).await?;

    let bid = relay_mux.fetch_best_bid(&bid_request).await?;

    validate_bid(&bid).await?;

    Ok(Json(VersionedValue {
        version: ConsensusVersion::Bellatrix,
        data: bid,
    }))
}

async fn handle_accept_bid(
    Json(block): Json<SignedBlindedBeaconBlock>,
    Extension(relay_mux): Extension<RelayMux>,
) -> Result<Json<VersionedValue<ExecutionPayload>>, Error> {
    tracing::debug!("accepting bid for block {block:?}");

    validate_signed_block(&block).await?;

    let payload = relay_mux.accept_bid(&block).await?;

    validate_execution_payload(&payload).await?;

    Ok(Json(VersionedValue {
        version: ConsensusVersion::Bellatrix,
        data: payload,
    }))
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
            .route("/eth/v1/builder/status", get(handle_status_check))
            .route(
                "/eth/v1/builder/validators",
                post(handle_validator_registration),
            )
            .route(
                "/eth/v1/builder/header/:slot/:parent_hash/:public_key",
                get(handle_fetch_bid),
            )
            .route("/eth/v1/builder/blinded_blocks", post(handle_accept_bid))
            .layer(Extension(relay_mux));
        let addr = SocketAddr::from((self.host, self.port));
        let json_rpc_handler = axum::Server::bind(&addr).serve(router.into_make_service());

        tracing::info!("listening at {addr}...");
        if let Err(err) = json_rpc_handler.await {
            tracing::error!("error while listening for incoming: {err}")
        }
    }
}
