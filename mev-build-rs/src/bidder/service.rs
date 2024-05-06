use crate::{
    auctioneer::AuctionContext,
    bidder::{strategies::BasicStrategy, Config},
};
use reth::{primitives::U256, tasks::TaskExecutor};
use std::sync::Arc;
use tokio::sync::{mpsc::Receiver, oneshot};
use tracing::trace;

pub type RevenueUpdate = (U256, oneshot::Sender<Option<U256>>);

pub struct Service {
    executor: TaskExecutor,
    config: Config,
}

impl Service {
    pub fn new(executor: TaskExecutor, config: Config) -> Self {
        Self { executor, config }
    }

    pub fn start_bid(
        &self,
        auction: Arc<AuctionContext>,
        mut revenue_updates: Receiver<RevenueUpdate>,
    ) {
        // TODO: make strategies configurable...
        let mut strategy = BasicStrategy::new(&self.config);
        self.executor.spawn_blocking(async move {
            // NOTE: `revenue_updates` will be closed when the builder is done with new payloads for
            // this auction so we can just loop on `recv` and return naturally once the
            // channel is closed
            while let Some((current_revenue, dispatch)) = revenue_updates.recv().await {
                let value = strategy.run(&auction, current_revenue).await;
                if dispatch.send(value).is_err() {
                    trace!("channel closed; could not send bid value to builder");
                    break
                }
            }
        });
    }
}
