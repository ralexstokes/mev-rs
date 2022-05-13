use crate::types::{
    BidRequest, BlsPublicKey, BuilderBid, ExecutionAddress, ExecutionPayload,
    ExecutionPayloadHeader, SignedBlindedBeaconBlock, SignedBuilderBid,
    SignedValidatorRegistration, U256,
};
use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use beacon_api_client::{ApiError, ConsensusVersion, VersionedValue};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
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
    #[error("internal server error")]
    Internal,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let message = self.to_string();
        let code = match self {
            Self::UnknownHash => StatusCode::BAD_REQUEST,
            Self::UnknownValidator => StatusCode::BAD_REQUEST,
            Self::UnknownFeeRecipient => StatusCode::BAD_REQUEST,
            Self::UnknownBlock => StatusCode::BAD_REQUEST,
            Self::InvalidSignature => StatusCode::BAD_REQUEST,
            Self::InvalidTimestamp => StatusCode::BAD_REQUEST,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (code, Json(ApiError { code, message })).into_response()
    }
}

async fn validate_registration(_registration: &SignedValidatorRegistration) -> Result<(), Error> {
    // TODO validations

    // track timestamps
    // -- must be greater than previous successful announcement
    // -- if more than 10 seconds in future, error

    // pubkey is active or in entry queue
    // -- `is_eligible_for_activation` || `is_active_validator`

    // verify signature
    Ok(())
}

async fn validate_bid_request(_bid_request: &BidRequest) -> Result<(), Error> {
    // TODO validations

    // verify slot is timely

    // verify parent_hash is on a chain tip

    // verify public_key is one of the possible proposers

    Ok(())
}

async fn validate_bid(_bid: &SignedBuilderBid) -> Result<(), Error> {
    // TODO validations

    // verify builder signature

    // OPTIONAL:
    // verify payload header
    // -- parent_hash matches
    // -- fee recip matches, maybe
    // -- prev_randao matches
    // -- block_number matches
    // -- gas_limit is valid
    // -- timestamp is valid
    // -- base_fee_per_gas makes sense

    Ok(())
}

async fn validate_signed_block(_signed_block: &SignedBlindedBeaconBlock) -> Result<(), Error> {
    // TODO validations

    // verify signature

    // OPTIONAL:
    // verify slot is timely
    // verify proposer_index is correct
    // verify parent_root matches
    // verify payload header matches the one we sent out
    Ok(())
}

async fn validate_execution_payload(_execution_payload: &ExecutionPayload) -> Result<(), Error> {
    // TODO validations

    // optional ish
    // verify root matches root of corresponding header that was accepted

    Ok(())
}

async fn handle_status_check() -> impl IntoResponse {
    tracing::debug!("status check");
    StatusCode::OK
}

async fn handle_validator_registration(
    Json(registration): Json<SignedValidatorRegistration>,
    Extension(state): Extension<Arc<Mutex<State>>>,
) -> Result<(), Error> {
    tracing::debug!("processing registration {registration:?}");

    validate_registration(&registration).await?;

    let registration = &registration.message;
    let mut state = state.lock().expect("can lock");
    state.fee_recipients.insert(
        registration.public_key.clone(),
        registration.fee_recipient.clone(),
    );

    tracing::debug!("{:?}", state);

    Ok(())
}

async fn handle_fetch_bid(
    Path(bid_request): Path<BidRequest>,
    Extension(state): Extension<Arc<Mutex<State>>>,
) -> Result<Json<VersionedValue<SignedBuilderBid>>, Error> {
    tracing::debug!("fetching best bid for block for request {bid_request:?}");

    validate_bid_request(&bid_request).await?;

    let public_key = &bid_request.public_key;

    let state = state.lock().unwrap();
    let fee_recipient = state
        .fee_recipients
        .get(public_key)
        .ok_or(Error::UnknownValidator)?;

    let bid = BuilderBid {
        header: ExecutionPayloadHeader {
            parent_hash: bid_request.parent_hash.clone(),
            fee_recipient: fee_recipient.clone(),
            ..Default::default()
        },
        value: U256::from_bytes_le([1u8; 32]),
        public_key: Default::default(),
    };

    let signed_bid = SignedBuilderBid {
        message: bid,
        ..Default::default()
    };

    // TODO validate?

    Ok(Json(VersionedValue {
        version: ConsensusVersion::Bellatrix,
        data: signed_bid,
    }))
}

async fn handle_accept_bid(
    Json(block): Json<SignedBlindedBeaconBlock>,
    Extension(state): Extension<Arc<Mutex<State>>>,
) -> Result<Json<VersionedValue<ExecutionPayload>>, Error> {
    tracing::debug!("accepting bid for block {block:?}");

    validate_signed_block(&block).await?;

    let block = &block.message;
    let header = &block.body.execution_payload_header;

    let payload = ExecutionPayload {
        parent_hash: header.parent_hash.clone(),
        fee_recipient: header.fee_recipient.clone(),
        ..Default::default()
    };

    validate_execution_payload(&payload).await?;

    Ok(Json(VersionedValue {
        version: ConsensusVersion::Bellatrix,
        data: payload,
    }))
}

pub struct Server {
    host: Ipv4Addr,
    port: u16,
    state: Arc<Mutex<State>>,
}

#[derive(Debug, Default)]
struct State {
    fee_recipients: HashMap<BlsPublicKey, ExecutionAddress>,
}

impl Server {
    pub fn new(host: Ipv4Addr, port: u16) -> Self {
        Self {
            host,
            port,
            state: Default::default(),
        }
    }

    pub async fn run(&self) {
        let state = self.state.clone();
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
            .layer(Extension(state));
        let addr = SocketAddr::from((self.host, self.port));
        let server = axum::Server::bind(&addr).serve(router.into_make_service());

        tracing::info!("relay server listening at {addr}...");
        if let Err(err) = server.await {
            tracing::error!("error while listening for incoming: {err}")
        }
    }
}
