use crate::{
    blinded_block_provider::{
        api::server::{
            handle_fetch_bid, handle_open_bid, handle_status_check, handle_validator_registration,
        },
        BlindedBlockProvider,
    },
    blinded_block_relayer::BlindedBlockRelayer,
    error::Error,
    types::{ProposerSchedule, SignedBidSubmission},
};
use axum::{
    extract::{Json, State},
    routing::{get, post, IntoMakeService},
    Router,
};
use hyper::server::conn::AddrIncoming;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::task::JoinHandle;
use tracing::{error, info, trace};

/// Type alias for the configured axum server
pub type BlockrelayServer = axum::Server<AddrIncoming, IntoMakeService<Router>>;

async fn handle_get_proposal_schedule<R: BlindedBlockRelayer>(
    State(relay): State<R>,
) -> Result<Json<Vec<ProposerSchedule>>, Error> {
    trace!("serving proposal schedule for current and next epoch");
    Ok(Json(relay.get_proposal_schedule().await?))
}

async fn handle_submit_bid<R: BlindedBlockRelayer>(
    State(relay): State<R>,
    Json(mut signed_bid_submission): Json<SignedBidSubmission>,
) -> Result<(), Error> {
    trace!("handling bid submission");
    relay.submit_bid(&mut signed_bid_submission).await
}

pub struct Server<R: BlindedBlockRelayer + BlindedBlockProvider> {
    host: Ipv4Addr,
    port: u16,
    relay: R,
}

impl<R: BlindedBlockRelayer + BlindedBlockProvider + Clone + Send + Sync + 'static> Server<R> {
    pub fn new(host: Ipv4Addr, port: u16, relay: R) -> Self {
        Self { host, port, relay }
    }

    /// Configures and returns the axum server
    pub fn serve(&self) -> BlockrelayServer {
        let router = Router::new()
            .route("/eth/v1/builder/status", get(handle_status_check))
            .route("/eth/v1/builder/validators", post(handle_validator_registration::<R>))
            .route(
                "/eth/v1/builder/header/:slot/:parent_hash/:public_key",
                get(handle_fetch_bid::<R>),
            )
            .route("/eth/v1/builder/blinded_blocks", post(handle_open_bid::<R>))
            .route("/relay/v1/builder/validators", get(handle_get_proposal_schedule::<R>))
            .route("/relay/v1/builder/blocks", post(handle_submit_bid::<R>))
            .with_state(self.relay.clone());
        let addr = SocketAddr::from((self.host, self.port));
        axum::Server::bind(&addr).serve(router.into_make_service())
    }

    /// Spawns the server on a new task returning the handle for it
    pub fn spawn(&self) -> JoinHandle<()> {
        let server = self.serve();
        let addr = server.local_addr();
        tokio::spawn(async move {
            info!("listening at {addr}...");
            if let Err(err) = server.await {
                error!(%err, "error while listening for incoming")
            }
        })
    }
}
