use crate::{
    blinded_block_relayer::BlindedBlockRelayer,
    error::Error,
    types::{ProposerSchedule, SignedBidReceipt, SignedBidSubmission},
};
use axum::{
    extract::{Json, Query, State},
    routing::{get, post, IntoMakeService},
    Router,
};
use hyper::server::conn::AddrIncoming;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::task::JoinHandle;

/// Type alias for the configured axum server
pub type BlockRelayerServer = axum::Server<AddrIncoming, IntoMakeService<Router>>;

async fn handle_get_proposal_schedule<R: BlindedBlockRelayer>(
    State(relayer): State<R>,
) -> Result<Json<Vec<ProposerSchedule>>, Error> {
    tracing::info!("serving proposal schedule for current and next epoch");
    Ok(Json(relayer.get_proposal_schedule().await?))
}

async fn handle_submit_bid<R: BlindedBlockRelayer>(
    State(relayer): State<R>,
    Query(with_cancellations): Query<bool>,
    Json(signed_bid_submission): Json<SignedBidSubmission>,
) -> Result<Json<SignedBidReceipt>, Error> {
    tracing::info!("handling bid submission");
    Ok(Json(relayer.submit_bid(&signed_bid_submission, with_cancellations).await?))
}

pub struct Server<R: BlindedBlockRelayer> {
    host: Ipv4Addr,
    port: u16,
    relayer: R,
}

impl<R: BlindedBlockRelayer + Clone + Send + Sync + 'static> Server<R> {
    pub fn new(host: Ipv4Addr, port: u16, relayer: R) -> Self {
        Self { host, port, relayer }
    }

    /// Configures and returns the axum server
    pub fn serve(&self) -> BlockRelayerServer {
        let router = Router::new()
            .route("/relay/v1/builder/validators", get(handle_get_proposal_schedule::<R>))
            .route("/relay/v1/builder/blocks", post(handle_submit_bid::<R>))
            .with_state(self.relayer.clone());
        let addr = SocketAddr::from((self.host, self.port));
        axum::Server::bind(&addr).serve(router.into_make_service())
    }

    /// Spawns the server on a new task returning the handle for it
    pub fn spawn(&self) -> JoinHandle<()> {
        let server = self.serve();
        let addr = server.local_addr();
        tokio::spawn(async move {
            tracing::info!("listening at {addr}...");
            if let Err(err) = server.await {
                tracing::error!("error while listening for incoming: {err}")
            }
        })
    }
}
