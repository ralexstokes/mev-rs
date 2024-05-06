use crate::{
    auctioneer::AuctionContext,
    bidder::{strategies::BasicStrategy, Config, KeepAlive},
};
use reth::{primitives::U256, tasks::TaskExecutor};
use std::sync::Arc;
use tokio::sync::{mpsc::Receiver, oneshot};

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
            while let Some((current_revenue, dispatch)) = revenue_updates.recv().await {
                let (value, keep_alive) = strategy.run(&auction, current_revenue).await;
                if dispatch.send(value).is_err() {
                    // builder is done
                    break
                }
                if matches!(keep_alive, KeepAlive::No) {
                    // close `builder` to signal bidding is done
                    break
                }
            }
        });
    }
}
