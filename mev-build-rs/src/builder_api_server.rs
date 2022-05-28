use crate::builder::Builder;
use crate::error::Error;
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
use beacon_api_client::{ApiError, ConsensusVersion, VersionedValue};
use std::net::{Ipv4Addr, SocketAddr};

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let message = self.to_string();
        let code = match self {
            Self::Internal(..) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::BAD_REQUEST,
        };
        (code, Json(ApiError { code, message })).into_response()
    }
}

async fn handle_status_check() -> impl IntoResponse {
    tracing::debug!("status check");
    StatusCode::OK
}

async fn handle_validator_registration<B: Builder>(
    Json(mut registrations): Json<Vec<SignedValidatorRegistration>>,
    Extension(builder): Extension<B>,
) -> Result<(), Error> {
    tracing::debug!("processing registrations {registrations:?}");

    builder
        .register_validator(&mut registrations)
        .await
        .map_err(From::from)
}

async fn handle_fetch_bid<B: Builder>(
    Path(bid_request): Path<BidRequest>,
    Extension(builder): Extension<B>,
) -> Result<Json<VersionedValue<SignedBuilderBid>>, Error> {
    tracing::debug!("fetching best bid for block for request {bid_request:?}");

    let signed_bid = builder.fetch_best_bid(&bid_request).await?;

    Ok(Json(VersionedValue {
        version: ConsensusVersion::Bellatrix,
        data: signed_bid,
    }))
}

async fn handle_open_bid<B: Builder>(
    Json(mut block): Json<SignedBlindedBeaconBlock>,
    Extension(builder): Extension<B>,
) -> Result<Json<VersionedValue<ExecutionPayload>>, Error> {
    tracing::debug!("opening bid for block {block:?}");

    let payload = builder.open_bid(&mut block).await?;

    Ok(Json(VersionedValue {
        version: ConsensusVersion::Bellatrix,
        data: payload,
    }))
}

pub struct Server<B: Builder> {
    host: Ipv4Addr,
    port: u16,
    builder: B,
}

impl<B: Builder + Clone + Send + Sync + 'static> Server<B> {
    pub fn new(host: Ipv4Addr, port: u16, builder: B) -> Self {
        Self {
            host,
            port,
            builder,
        }
    }

    pub async fn run(&self) {
        let router = Router::new()
            .route("/eth/v1/builder/status", get(handle_status_check))
            .route(
                "/eth/v1/builder/validators",
                post(handle_validator_registration::<B>),
            )
            .route(
                "/eth/v1/builder/header/:slot/:parent_hash/:public_key",
                get(handle_fetch_bid::<B>),
            )
            .route("/eth/v1/builder/blinded_blocks", post(handle_open_bid::<B>))
            .layer(Extension(self.builder.clone()));
        let addr = SocketAddr::from((self.host, self.port));
        let server = axum::Server::bind(&addr).serve(router.into_make_service());

        tracing::info!("listening at {addr}...");
        if let Err(err) = server.await {
            tracing::error!("error while listening for incoming: {err}")
        }
    }
}
