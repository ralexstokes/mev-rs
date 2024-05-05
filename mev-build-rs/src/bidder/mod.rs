mod service;
pub mod strategies;

pub use service::{Message, RevenueUpdate, Service};
pub use strategies::Config;

/// Do we expect to submit more bids or not?
#[derive(Debug, Clone, Copy)]
pub enum KeepAlive {
    Yes,
    // TODO: remove once used
    #[allow(unused)]
    No,
}
