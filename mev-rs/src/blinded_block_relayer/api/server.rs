use crate::{
    blinded_block_provider::{
        api::server::{
            handle_fetch_bid, handle_open_bid, handle_status_check, handle_validator_registration,
        },
        BlindedBlockProvider,
    },
    blinded_block_relayer::{
        BlindedBlockDataProvider, BlindedBlockRelayer, BlockSubmissionFilter,
        DeliveredPayloadFilter, ValidatorRegistrationQuery,
    },
    error::Error,
    types::{
        block_submission::data_api::{PayloadTrace, SubmissionTrace},
        ProposerSchedule, SignedBidSubmission, SignedValidatorRegistration,
    },
};
use axum::{
    extract::{Json, Query, State},
    routing::{get, post, IntoMakeService},
    Router,
};
use hyper::server::conn::AddrIncoming;
use std::net::{Ipv4Addr, SocketAddr};
use tokio::task::JoinHandle;
use tracing::{error, info, trace};

/// Type alias for the configured axum server
pub type BlockRelayServer = axum::Server<AddrIncoming, IntoMakeService<Router>>;

async fn handle_get_proposal_schedule<R: BlindedBlockRelayer>(
    State(relay): State<R>,
) -> Result<Json<Vec<ProposerSchedule>>, Error> {
    trace!("serving proposal schedule for current and next epoch");
    Ok(Json(relay.get_proposal_schedule().await?))
}

async fn handle_submit_bid<R: BlindedBlockRelayer>(
    State(relay): State<R>,
    Json(signed_bid_submission): Json<SignedBidSubmission>,
) -> Result<(), Error> {
    trace!("handling bid submission");
    relay.submit_bid(&signed_bid_submission).await
}

async fn handle_get_proposer_payloads_delivered<R: BlindedBlockDataProvider>(
    State(relay): State<R>,
    Query(filters): Query<DeliveredPayloadFilter>,
) -> Result<Json<Vec<PayloadTrace>>, Error> {
    trace!("handling proposer payloads delivered");
    Ok(Json(relay.get_delivered_payloads(&filters).await?))
}

async fn handle_get_builder_blocks_received<R: BlindedBlockDataProvider>(
    State(relay): State<R>,
    Query(filters): Query<BlockSubmissionFilter>,
) -> Result<Json<Vec<SubmissionTrace>>, Error> {
    trace!("handling block submissions");
    Ok(Json(relay.get_block_submissions(&filters).await?))
}

async fn handle_get_validator_registration<R: BlindedBlockDataProvider>(
    State(relay): State<R>,
    Query(params): Query<ValidatorRegistrationQuery>,
) -> Result<Json<SignedValidatorRegistration>, Error> {
    trace!("handling fetch validator registration");
    Ok(Json(relay.fetch_validator_registration(&params.public_key).await?))
}

pub struct Server<R> {
    host: Ipv4Addr,
    port: u16,
    relay: R,
}

impl<
        R: BlindedBlockRelayer
            + BlindedBlockProvider
            + BlindedBlockDataProvider
            + Clone
            + Send
            + Sync
            + 'static,
    > Server<R>
{
    pub fn new(host: Ipv4Addr, port: u16, relay: R) -> Self {
        Self { host, port, relay }
    }

    /// Configures and returns the axum server
    pub fn serve(&self) -> BlockRelayServer {
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
            .route(
                "/relay/v1/data/bidtraces/proposer_payload_delivered",
                get(handle_get_proposer_payloads_delivered::<R>),
            )
            .route(
                "/relay/v1/data/bidtraces/builder_blocks_received",
                get(handle_get_builder_blocks_received::<R>),
            )
            .route(
                "/relay/v1/data/validator_registration",
                get(handle_get_validator_registration::<R>),
            )
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
