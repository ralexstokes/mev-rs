use std::sync::{Once, OnceLock};

use ethereum_consensus::primitives::BlsPublicKey;
use prometheus::{
    register_histogram_vec, register_int_counter_vec, HistogramOpts, HistogramVec, IntCounterVec,
    Opts, DEFAULT_BUCKETS,
};

const NAMESPACE: &str = "boost";
const SUBSYSTEM: &str = "builder";

const API_METHOD_LABEL: &str = "method";
const RELAY_LABEL: &str = "relay";

pub static API_REQUESTS_COUNTER: OnceLock<IntCounterVec> = OnceLock::new();
pub static API_TIMEOUT_COUNTER: OnceLock<IntCounterVec> = OnceLock::new();
pub static API_REQUEST_DURATION_SECONDS: OnceLock<HistogramVec> = OnceLock::new();

pub static AUCTION_INVALID_BIDS_COUNTER: OnceLock<IntCounterVec> = OnceLock::new();

static INIT: Once = Once::new();

pub(crate) fn init() {
    INIT.call_once(|| {
        API_REQUESTS_COUNTER
            .set(
                register_int_counter_vec!(
                    Opts::new("api_requests_total", "total number of builder API requests")
                        .namespace(NAMESPACE)
                        .subsystem(SUBSYSTEM),
                    &[API_METHOD_LABEL, RELAY_LABEL]
                )
                .unwrap(),
            )
            .unwrap();

        API_TIMEOUT_COUNTER
            .set(
                register_int_counter_vec!(
                    Opts::new("api_timeouts_total", "total number of builder API timeouts")
                        .namespace(NAMESPACE)
                        .subsystem(SUBSYSTEM),
                    &[API_METHOD_LABEL, RELAY_LABEL]
                )
                .unwrap(),
            )
            .unwrap();
        API_REQUEST_DURATION_SECONDS
            .set(
                register_histogram_vec!(
                    HistogramOpts {
                        common_opts: Opts::new(
                            "api_request_duration_seconds",
                            "duration (in seconds) of builder API timeouts"
                        )
                        .namespace(NAMESPACE)
                        .subsystem(SUBSYSTEM),
                        buckets: DEFAULT_BUCKETS.to_vec(),
                    },
                    &[API_METHOD_LABEL, RELAY_LABEL]
                )
                .unwrap(),
            )
            .unwrap();

        AUCTION_INVALID_BIDS_COUNTER
            .set(
                register_int_counter_vec!(
                    Opts::new("auction_invalid_bids_total", "total number of invalid builder bids")
                        .namespace(NAMESPACE)
                        .subsystem(SUBSYSTEM),
                    &[RELAY_LABEL]
                )
                .unwrap(),
            )
            .unwrap();
    });
}

pub fn inc_api_int_counter_vec(
    counter_vec: &OnceLock<IntCounterVec>,
    meth: ApiMethod,
    relay: &BlsPublicKey,
) {
    if let Some(counter) = counter_vec.get() {
        counter.with_label_values(&[meth.as_str(), &relay.to_string()]).inc();
    }
}

pub fn observe_api_histogram_vec(
    hist_vec: &OnceLock<HistogramVec>,
    meth: ApiMethod,
    relay: &BlsPublicKey,
    obs: f64,
) {
    if let Some(hist) = hist_vec.get() {
        hist.with_label_values(&[meth.as_str(), &relay.to_string()]).observe(obs);
    }
}

pub fn inc_auction_int_counter_vec(counter_vec: &OnceLock<IntCounterVec>, relay: &BlsPublicKey) {
    if let Some(counter) = counter_vec.get() {
        counter.with_label_values(&[&relay.to_string()]).inc();
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ApiMethod {
    Register,
    GetHeader,
    GetPayload,
}

impl ApiMethod {
    pub const fn as_str(&self) -> &str {
        match self {
            Self::Register => "register",
            Self::GetHeader => "get_header",
            Self::GetPayload => "get_payload",
        }
    }
}
