use ethereum_consensus::Error as ConsensusError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
}
