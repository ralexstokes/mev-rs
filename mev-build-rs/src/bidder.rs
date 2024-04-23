use crate::{
    auction_schedule::{Proposer, RelaySet},
    utils::payload_job::duration_until,
};
use ethereum_consensus::primitives::Slot;
use reth::payload::{EthPayloadBuilderAttributes, PayloadId};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug)]
pub struct AuctionContext {
    pub slot: Slot,
    pub attributes: EthPayloadBuilderAttributes,
    pub proposer: Proposer,
    pub relays: RelaySet,
}

/// `DeadlineBidder` submits the best payload *once* at the `deadline`
/// expressed as a `Duration` *before* the start of the build's target slot.
///
/// For example, if the `deadline` is 1 second, then the bidder will return
/// a value to bid one second before the start of the build's target slot.
pub struct DeadlineBidder {
    deadline: Duration,
}

pub enum BidRequest {
    Ready(PayloadId),
}

impl DeadlineBidder {
    pub fn new(deadline: Duration) -> Self {
        Self { deadline }
    }

    pub async fn make_bid(&self, auction: &AuctionContext) -> BidRequest {
        let target = duration_until(auction.attributes.timestamp);
        let duration = target.checked_sub(self.deadline).unwrap_or_default();
        sleep(duration).await;
        BidRequest::Ready(auction.attributes.payload_id())
    }
}
