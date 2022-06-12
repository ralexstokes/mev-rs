use crate::blinded_block_provider::{BlindedBlockProvider, Error};
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
use beacon_api_client::{ApiError, ConsensusVersion, Value};
use std::collections::HashMap;
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

async fn handle_validator_registration<B: BlindedBlockProvider>(
    Json(mut registrations): Json<Vec<SignedValidatorRegistration>>,
    Extension(builder): Extension<B>,
) -> Result<(), Error> {
    tracing::debug!("processing registrations {registrations:?}");

    builder
        .register_validators(&mut registrations)
        .await
        .map_err(From::from)
}

async fn handle_fetch_bid<B: BlindedBlockProvider>(
    Path(bid_request): Path<BidRequest>,
    Extension(builder): Extension<B>,
) -> Result<Json<Value<SignedBuilderBid>>, Error> {
    tracing::debug!("fetching best bid for block for request {bid_request:?}");

    let signed_bid = builder.fetch_best_bid(&bid_request).await?;

    let version = serde_json::to_value(ConsensusVersion::Bellatrix).unwrap();
    Ok(Json(Value {
        meta: HashMap::from_iter([("version".to_string(), version)]),
        data: signed_bid,
    }))
}

async fn handle_open_bid<B: BlindedBlockProvider>(
    Json(mut block): Json<SignedBlindedBeaconBlock>,
    Extension(builder): Extension<B>,
) -> Result<Json<Value<ExecutionPayload>>, Error> {
    tracing::debug!("opening bid for block {block:?}");

    let payload = builder.open_bid(&mut block).await?;

    let version = serde_json::to_value(ConsensusVersion::Bellatrix).unwrap();
    Ok(Json(Value {
        meta: HashMap::from_iter([("version".to_string(), version)]),
        data: payload,
    }))
}

pub struct Server<B: BlindedBlockProvider> {
    host: Ipv4Addr,
    port: u16,
    builder: B,
}

impl<B: BlindedBlockProvider + Clone + Send + Sync + 'static> Server<B> {
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
