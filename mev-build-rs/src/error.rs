use alloy_signer_local::LocalSignerError;
use ethereum_consensus::{Error as ConsensusError, Fork};
use reth::payload::PayloadBuilderError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("fork {0} is not supported for this operation")]
    UnsupportedFork(Fork),
    #[error(transparent)]
    Consensus(#[from] ConsensusError),
    #[error(transparent)]
    PayloadBuilderError(#[from] PayloadBuilderError),
    #[error(transparent)]
    WalletError(#[from] LocalSignerError),
}
