use crate::{
    blinded_block_provider::{BlindedBlockProvider, Error},
    types::{
        BidRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
};
use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post, IntoMakeService},
    Router,
};
use beacon_api_client::{ApiError, VersionedValue};
use hyper::server::conn::AddrIncoming;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::task::JoinHandle;

/// Type alias for the configured axum server
pub type BlockProviderServer = axum::Server<AddrIncoming, IntoMakeService<Router>>;

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

    builder.register_validators(&mut registrations).await.map_err(From::from)
}

async fn handle_fetch_bid<B: BlindedBlockProvider>(
    Path(bid_request): Path<BidRequest>,
    Extension(builder): Extension<B>,
) -> Result<Json<VersionedValue<SignedBuilderBid>>, Error> {
    tracing::debug!("fetching best bid for block for request {bid_request:?}");

    let signed_bid = builder.fetch_best_bid(&bid_request).await?;

    let response = VersionedValue { payload: signed_bid, meta: Default::default() };
    Ok(Json(response))
}

async fn handle_open_bid<B: BlindedBlockProvider>(
    Json(mut block): Json<SignedBlindedBeaconBlock>,
    Extension(builder): Extension<B>,
) -> Result<Json<VersionedValue<ExecutionPayload>>, Error> {
    tracing::debug!("opening bid for block {block:?}");

    let payload = builder.open_bid(&mut block).await?;

    let response = VersionedValue { payload, meta: Default::default() };
    Ok(Json(response))
}

pub struct Server<B: BlindedBlockProvider> {
    host: Ipv4Addr,
    port: u16,
    builder: B,
}

impl<B: BlindedBlockProvider + Clone + Send + Sync + 'static> Server<B> {
    pub fn new(host: Ipv4Addr, port: u16, builder: B) -> Self {
        Self { host, port, builder }
    }

    /// Configures and returns the axum server
    pub fn serve(&self) -> BlockProviderServer {
        let router = Router::new()
            .route("/eth/v1/builder/status", get(handle_status_check))
            .route("/eth/v1/builder/validators", post(handle_validator_registration::<B>))
            .route(
                "/eth/v1/builder/header/:slot/:parent_hash/:public_key",
                get(handle_fetch_bid::<B>),
            )
            .route("/eth/v1/builder/blinded_blocks", post(handle_open_bid::<B>))
            .layer(Extension(self.builder.clone()));
        let addr = SocketAddr::from((self.host, self.port));
        axum::Server::bind(&addr).serve(router.into_make_service())
    }

    /// Spawns the server on a new task returning the handle for it
    pub fn spawn(&self) -> JoinHandle<()> {
        let server = self.serve();
        let address = server.local_addr();
        tokio::spawn(async move {
            tracing::info!("listening at {address}...");
            if let Err(err) = server.await {
                tracing::error!("error while listening for incoming: {err}")
            }
        })
    }
}
