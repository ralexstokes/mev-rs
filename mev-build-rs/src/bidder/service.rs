use crate::{
    auctioneer::AuctionContext,
    bidder::{strategies::BasicStrategy, Config, KeepAlive},
};
use ethereum_consensus::clock::duration_until;
use reth::{api::PayloadBuilderAttributes, primitives::U256, tasks::TaskExecutor};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{mpsc::Receiver, oneshot},
    time::timeout,
};

/// All bidding routines stop this many seconds *after* the timestamp of the proposal
/// regardless of what the bidding strategy suggests
pub const DEFAULT_BIDDING_DEADLINE_AFTER_SLOT: u64 = 1;

pub type RevenueUpdate = (U256, oneshot::Sender<Option<U256>>);

pub enum Message {
    NewAuction(Arc<AuctionContext>, Receiver<RevenueUpdate>),
}

pub struct Service {
    auctioneer: Receiver<Message>,
    executor: TaskExecutor,
    config: Config,
}

impl Service {
    pub fn new(auctioneer: Receiver<Message>, executor: TaskExecutor, config: Config) -> Self {
        Self { auctioneer, executor, config }
    }

    fn start_bid(
        &mut self,
        auction: Arc<AuctionContext>,
        mut revenue_updates: Receiver<RevenueUpdate>,
    ) {
        // TODO: make strategies configurable...
        let mut strategy = BasicStrategy::new(&self.config);
        let duration_after_slot = Duration::from_secs(DEFAULT_BIDDING_DEADLINE_AFTER_SLOT);
        let max_bidding_duration = duration_until(auction.attributes.timestamp())
            .checked_add(duration_after_slot)
            .unwrap_or_default();
        self.executor.spawn_blocking(async move {
            // TODO issues with timeout and open channels?
            let _ = timeout(max_bidding_duration, async move {
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
            })
            .await;
        });
    }

    pub async fn spawn(mut self) {
        while let Some(Message::NewAuction(auction, revenue_updates)) = self.auctioneer.recv().await
        {
            self.start_bid(auction, revenue_updates);
        }
    }
}
