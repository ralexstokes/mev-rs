use crate::{
    auctioneer::AuctionContext,
    bidder::{Bid, KeepAlive},
};
use ethereum_consensus::clock::duration_until;
use reth::{api::PayloadBuilderAttributes, primitives::U256};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::{interval, Interval};

pub const DEFAULT_BID_INTERVAL: u64 = 1;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    // amount in milliseconds of time to wait until submitting bids
    pub wait_until_ms: u64,
    // amount to bid as a fraction of the block's value
    // if missing, default to 100%
    pub bid_percent: Option<f64>,
    // amount to add from the builder's wallet as a subsidy to the auction bid
    // if missing, defaults to 0
    pub subsidy_wei: Option<U256>,
}

/// `DeadlineBidder` submits the best payload *once* at the `deadline`
/// expressed as a `Duration` *before* the start of the build's target slot.
///
/// For example, if the `deadline` is 1 second, then the bidder will return
/// a value to bid one second before the start of the build's target slot.
pub struct BasicStrategy {
    wait_until: Duration,
    bid_interval: Interval,
    bid_percent: f64,
    subsidy_wei: U256,
}

impl BasicStrategy {
    pub fn new(config: &Config) -> Self {
        let wait_until = Duration::from_millis(config.wait_until_ms);
        let bid_interval = interval(Duration::from_secs(DEFAULT_BID_INTERVAL));
        Self {
            wait_until,
            bid_interval,
            bid_percent: config.bid_percent.unwrap_or(1.0).clamp(0.0, 1.0),
            subsidy_wei: config.subsidy_wei.unwrap_or(U256::ZERO),
        }
    }

    fn compute_value(&self, current_revenue: U256) -> U256 {
        let mut value = current_revenue * U256::from(self.bid_percent * 100.0) / U256::from(100);
        value += self.subsidy_wei;
        value
    }

    pub async fn run(&mut self, auction: &AuctionContext, current_revenue: U256) -> Bid {
        // First, we wait until we are near the auction deadline
        let target = duration_until(auction.attributes.timestamp());
        let wait_until = target.checked_sub(self.wait_until).unwrap_or_default();
        if !wait_until.is_zero() {
            return Bid::Wait(wait_until)
        }

        // If we are near the auction deadline, start submitting bids
        // with one bid per tick of the interval
        self.bid_interval.tick().await;

        let value = self.compute_value(current_revenue);
        Bid::Submit { value, keep_alive: KeepAlive::Yes }
    }
}
