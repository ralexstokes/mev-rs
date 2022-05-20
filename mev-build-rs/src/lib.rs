mod builder;
#[cfg(feature = "api")]
mod builder_api_server;
#[cfg(feature = "api")]
mod client;
mod error;
#[cfg(feature = "serde")]
mod serde;
mod signing;
mod types;

#[cfg(feature = "api")]
pub use beacon_api_client::Error as ClientError;
pub use builder::Builder;
#[cfg(feature = "api")]
pub use builder_api_server::Server as ApiServer;
#[cfg(feature = "api")]
pub use client::Client as ApiClient;
pub use error::Error;
pub use signing::{
    sign_builder_message, verify_signed_builder_message, verify_signed_consensus_message,
};
pub use types::*;
