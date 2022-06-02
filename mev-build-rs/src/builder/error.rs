use crate::types::BidRequest as PayloadRequest;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no payload prepared for request: {0:?}")]
    NoPayloadPrepared(PayloadRequest),
}
