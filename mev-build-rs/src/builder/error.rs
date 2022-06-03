use crate::types::BidRequest as PayloadRequest;
use ethereum_consensus::primitives::BlsPublicKey;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("missing preferences for validator with public key {0}")]
    MissingPreferences(BlsPublicKey),
    #[error("no payload prepared for request: {0:?}")]
    NoPayloadPrepared(PayloadRequest),
}
