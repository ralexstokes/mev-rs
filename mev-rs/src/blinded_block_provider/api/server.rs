use crate::{
    blinded_block_provider::BlindedBlockProvider,
    error::Error,
    types::{
        AuctionRequest, ExecutionPayload, SignedBlindedBeaconBlock, SignedValidatorRegistration,
    },
};
use axum::{
    extract::{Json, Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post, IntoMakeService},
    Router,
};
use beacon_api_client::VersionedValue;
use hyper::server::conn::AddrIncoming;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::task::JoinHandle;
use tracing::{error, info, trace};

/// Type alias for the configured axum server
pub type BlockProviderServer = axum::Server<AddrIncoming, IntoMakeService<Router>>;

async fn handle_status_check() -> impl IntoResponse {
    StatusCode::OK
}

async fn handle_validator_registration<B: BlindedBlockProvider>(
    State(builder): State<B>,
    Json(mut registrations): Json<Vec<SignedValidatorRegistration>>,
) -> Result<(), Error> {
    trace!(count = registrations.len(), "processing validator registrations");
    builder.register_validators(&mut registrations).await.map_err(From::from)
}

async fn handle_fetch_bid<B: BlindedBlockProvider>(
    State(builder): State<B>,
    Path(auction_request): Path<AuctionRequest>,
) -> (StatusCode, String) {
    let signed_bid = match builder.fetch_best_bid(&auction_request).await {
        Ok(signed_bid) => signed_bid,
        Err(Error::NoBidPrepared(..)) => return (StatusCode::NO_CONTENT, Default::default()),
        Err(err) => return (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };
    trace!(%auction_request, %signed_bid, "returning bid");
    let version = signed_bid.version();
    let response = VersionedValue { version, data: signed_bid, meta: Default::default() };
    let response_json = serde_json::to_string(&response).unwrap();
    (StatusCode::OK, response_json)
}

async fn handle_open_bid<B: BlindedBlockProvider>(
    State(builder): State<B>,
    Json(mut block): Json<SignedBlindedBeaconBlock>,
) -> Result<Json<VersionedValue<ExecutionPayload>>, Error> {
    let payload = builder.open_bid(&mut block).await?;
    let block_hash = payload.block_hash();
    let slot = block.message().slot();
    trace!(%slot, %block_hash, "returning payload");
    let version = payload.version();
    let response = VersionedValue { version, data: payload, meta: Default::default() };
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
            .with_state(self.builder.clone());
        let addr = SocketAddr::from((self.host, self.port));
        axum::Server::bind(&addr).serve(router.into_make_service())
    }

    /// Spawns the server on a new task returning the handle for it
    pub fn spawn(&self) -> JoinHandle<()> {
        let server = self.serve();
        let address = server.local_addr();
        tokio::spawn(async move {
            info!("listening at {address}...");
            if let Err(err) = server.await {
                error!(%err, "error while listening for incoming")
            }
        })
    }
}
