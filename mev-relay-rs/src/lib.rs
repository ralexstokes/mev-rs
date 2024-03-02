mod relay;
mod service;
mod reth_cli_ext;
mod rpc;

use std::sync::Arc;
pub use service::{Config, Service};
use rpc::ValidationApiInner;

pub struct ValidationApi<Provider> {
    inner: Arc<ValidationApiInner<Provider>>,
}
