use crate::{
    auctioneer::AuctionContext,
    bidder::{BidStatus, KeepAlive},
};
use ethereum_consensus::clock::duration_until;
use reth::{api::PayloadBuilderAttributes, primitives::U256};
use serde::Deserialize;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    // amount in milliseconds
    pub bidding_deadline_ms: u64,
    // amount to bid as a fraction of the block's value
    // if missing, default to 100%
    // TODO: use to price bid
    pub bid_percent: Option<f64>,
    // amount to add from the builder's wallet as a subsidy to the auction bid
    // TODO: use to adjust bid
    pub subsidy_wei: Option<U256>,
}

/// `DeadlineBidder` submits the best payload *once* at the `deadline`
/// expressed as a `Duration` *before* the start of the build's target slot.
///
/// For example, if the `deadline` is 1 second, then the bidder will return
/// a value to bid one second before the start of the build's target slot.
pub struct DeadlineBidder {
    deadline: Duration,
    bid_percent: f64,
    subsidy_wei: U256,
}

impl DeadlineBidder {
    pub fn new(config: &Config) -> Self {
        let deadline = Duration::from_millis(config.bidding_deadline_ms);
        Self {
            deadline,
            bid_percent: config.bid_percent.unwrap_or(1.0).clamp(0.0, 1.0),
            subsidy_wei: config.subsidy_wei.unwrap_or(U256::ZERO),
        }
    }

    fn compute_value(&self, current_revenue: U256) -> U256 {
        let mut value = current_revenue * U256::from(self.bid_percent * 100.0) / U256::from(100);
        value += self.subsidy_wei;
        value
    }

    pub async fn run(&mut self, auction: &AuctionContext, current_revenue: U256) -> BidStatus {
        let value = self.compute_value(current_revenue);
        let target = duration_until(auction.attributes.timestamp());
        let duration = target.checked_sub(self.deadline).unwrap_or_default();
        sleep(duration).await;
        BidStatus::Submit { value, keep_alive: KeepAlive::No }
    }
}
