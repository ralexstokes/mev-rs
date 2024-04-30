mod service;
pub mod strategies;

use std::time::Duration;

use reth::primitives::U256;
pub use service::{Message, Service};
pub use strategies::Config;

/// Do we expect to submit more bids or not?
#[derive(Debug, Clone, Copy)]
pub enum KeepAlive {
    Yes,
}

pub enum Bid {
    Wait(Duration),
    Submit { value: U256, keep_alive: KeepAlive },
}
