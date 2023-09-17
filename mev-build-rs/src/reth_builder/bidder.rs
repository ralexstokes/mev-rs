use crate::reth_builder::{build::Build, error::Error};
use async_trait::async_trait;
use ethereum_consensus::clock::SystemClock;
use reth_transaction_pool::TransactionPool;
use std::time::Duration;

pub enum Bid {
    Continue,
    Done,
}

#[async_trait]
pub trait Bidder<Pool> {
    // Determine if a bid should be made given the current state of the `build`.
    // In a context where blocking is ok.
    async fn bid_for(&self, build: &Build<Pool>) -> Result<Option<Bid>, Error>;
}

/// `DeadlineBidder` submits the best payload *once* at the `deadline`
/// expressed as a `Duration` *before* the start of the build's target slot.
///
/// For example, if the `deadline` is 1 second, then the bidder will return
/// a value to bid one second before the start of the build's target slot.
pub struct DeadlineBidder {
    clock: SystemClock,
    deadline: Duration,
}

impl DeadlineBidder {
    pub fn new(clock: SystemClock, deadline: Duration) -> Self {
        Self { clock, deadline }
    }
}

#[async_trait]
impl<Pool> Bidder<Pool> for DeadlineBidder
where
    Pool: TransactionPool + Send + Sync + 'static,
{
    async fn bid_for(&self, build: &Build<Pool>) -> Result<Option<Bid>, Error> {
        let slot = build.context.slot;
        let target = self.clock.duration_until_slot(slot);
        let duration = target.checked_sub(self.deadline).unwrap_or_default();
        let id = build.context.id();
        tracing::debug!(%id, slot, ?duration, "waiting to submit bid");
        tokio::time::sleep(duration).await;

        Ok(Some(Bid::Done))
    }
}
