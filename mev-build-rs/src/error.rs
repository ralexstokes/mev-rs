use beacon_api_client::ApiError;
#[cfg(feature = "api")]
use beacon_api_client::Error as ApiClientError;
use ethereum_consensus::state_transition::Error as ConsensusError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Consensus(#[from] ConsensusError),
    #[error("{0}")]
    Api(#[from] ApiError),
    #[error("internal server error")]
    Internal(String),
    #[error("{0}")]
    Custom(String),
}

#[cfg(feature = "api")]
impl From<ApiClientError> for Error {
    fn from(err: ApiClientError) -> Self {
        match err {
            ApiClientError::Api(err) => err.into(),
            err => Error::Internal(err.to_string()),
        }
    }
}
