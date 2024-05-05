use crate::{auctioneer::AuctionContext, bidder::KeepAlive};
use reth::primitives::U256;
use serde::Deserialize;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    // amount to bid as a fraction of the block's value
    // if missing, default to 100%
    pub bid_percent: Option<f64>,
    // amount to add from the builder's wallet as a subsidy to the auction bid
    // if missing, defaults to 0
    pub subsidy_wei: Option<U256>,
}

/// `BasicStrategy` submits a bid for each built payload, with configurable options for:
/// - percent of the revenue to bid
/// - a "subsidy" to add
pub struct BasicStrategy {
    bid_percent: f64,
    subsidy_wei: U256,
}

impl BasicStrategy {
    pub fn new(config: &Config) -> Self {
        Self {
            bid_percent: config.bid_percent.unwrap_or(1.0).clamp(0.0, 1.0),
            subsidy_wei: config.subsidy_wei.unwrap_or_default(),
        }
    }

    fn compute_value(&self, current_revenue: U256) -> U256 {
        let mut value = current_revenue * U256::from(self.bid_percent * 100.0) / U256::from(100);
        value += self.subsidy_wei;
        value
    }

    pub async fn run(
        &mut self,
        _auction: &AuctionContext,
        current_revenue: U256,
    ) -> (Option<U256>, KeepAlive) {
        let value = self.compute_value(current_revenue);
        (Some(value), KeepAlive::Yes)
    }
}
