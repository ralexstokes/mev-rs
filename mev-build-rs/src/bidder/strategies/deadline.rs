use crate::{
    auctioneer::AuctionContext,
    bidder::{BidStatus, KeepAlive},
};
use ethereum_consensus::clock::duration_until;
use reth::api::PayloadBuilderAttributes;
use std::time::Duration;
use tokio::time::sleep;

/// `DeadlineBidder` submits the best payload *once* at the `deadline`
/// expressed as a `Duration` *before* the start of the build's target slot.
///
/// For example, if the `deadline` is 1 second, then the bidder will return
/// a value to bid one second before the start of the build's target slot.
pub struct DeadlineBidder {
    deadline: Duration,
}

impl DeadlineBidder {
    pub fn new(deadline: Duration) -> Self {
        Self { deadline }
    }

    pub async fn run(&mut self, auction: &AuctionContext) -> BidStatus {
        let target = duration_until(auction.attributes.timestamp());
        let duration = target.checked_sub(self.deadline).unwrap_or_default();
        sleep(duration).await;
        BidStatus::Dispatch(auction.attributes.payload_id(), KeepAlive::No)
    }
}
