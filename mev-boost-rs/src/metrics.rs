use std::ops::Deref;

use ethereum_consensus::primitives::BlsPublicKey;
use lazy_static::lazy_static;
use prometheus::{
    register_histogram_vec, register_int_counter_vec, HistogramOpts, HistogramVec, IntCounterVec,
    Opts, DEFAULT_BUCKETS,
};

const NAMESPACE: &str = "boost";
const SUBSYSTEM: &str = "builder";

const API_METHOD_LABEL: &str = "method";
const RELAY_LABEL: &str = "relay";

lazy_static! {
    pub static ref API_REQUESTS_COUNTER: IntCounterVec = register_int_counter_vec!(
        Opts::new("api_requests_total", "total number of builder API requests")
            .namespace(NAMESPACE)
            .subsystem(SUBSYSTEM),
        &[API_METHOD_LABEL, RELAY_LABEL]
    )
    .unwrap();
    pub static ref API_TIMEOUT_COUNTER: IntCounterVec = register_int_counter_vec!(
        Opts::new("api_timeouts_total", "total number of builder API timeouts")
            .namespace(NAMESPACE)
            .subsystem(SUBSYSTEM),
        &[API_METHOD_LABEL, RELAY_LABEL]
    )
    .unwrap();
    pub static ref API_REQUEST_DURATION_SECONDS: HistogramVec = register_histogram_vec!(
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
    .unwrap();
    pub static ref AUCTION_INVALID_BIDS_COUNTER: IntCounterVec = register_int_counter_vec!(
        Opts::new("auction_invalid_bids_total", "total number of invalid builder bids")
            .namespace(NAMESPACE)
            .subsystem(SUBSYSTEM),
        &[RELAY_LABEL]
    )
    .unwrap();
}

pub fn inc_api_int_counter_vec<C: Deref<Target = IntCounterVec>>(
    counter_vec: &C,
    meth: ApiMethod,
    relay: &BlsPublicKey,
) {
    counter_vec.with_label_values(&[meth.as_str(), &relay.to_string()]).inc();
}

pub fn observe_api_histogram_vec<H: Deref<Target = HistogramVec>>(
    hist_vec: &H,
    meth: ApiMethod,
    relay: &BlsPublicKey,
    obs: f64,
) {
    hist_vec.with_label_values(&[meth.as_str(), &relay.to_string()]).observe(obs);
}

pub fn inc_auction_int_counter_vec<C: Deref<Target = IntCounterVec>>(
    counter_vec: &C,
    relay: &BlsPublicKey,
) {
    counter_vec.with_label_values(&[&relay.to_string()]).inc();
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
