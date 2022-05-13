use crate::relay_mux::{Error as RelayMuxError, RelayMux};
use crate::types::{
    BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration,
};
use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use beacon_api_client::{ApiError, ConsensusVersion, Error as BeaconApiError, VersionedValue};
use std::net::{Ipv4Addr, SocketAddr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Relay(#[from] RelayMuxError),
    #[error("internal server error")]
    Internal(String),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let message = self.to_string();
        let code = match self {
            Self::Relay(err) => match err {
                RelayMuxError::Relay(BeaconApiError::Api(api_err)) => {
                    return (api_err.code, Json(api_err)).into_response();
                }
                _ => StatusCode::BAD_REQUEST,
            },
            Self::Internal(..) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (code, Json(ApiError { code, message })).into_response()
    }
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

    relay_mux.register_validator(&registration).await?;

    Ok(())
}

async fn handle_fetch_bid(
    Path(bid_request): Path<BidRequest>,
    Extension(relay_mux): Extension<RelayMux>,
) -> Result<Json<VersionedValue<SignedBuilderBid>>, Error> {
    tracing::debug!("fetching best bid for block for request {bid_request:?}");

    let bid = relay_mux.fetch_best_bid(&bid_request).await?;

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

    let payload = relay_mux.accept_bid(&block).await?;

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

    pub async fn run(&self, relay_mux: RelayMux) {
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
        let server = axum::Server::bind(&addr).serve(router.into_make_service());

        tracing::info!("listening at {addr}...");
        if let Err(err) = server.await {
            tracing::error!("error while listening for incoming: {err}")
        }
    }
}
