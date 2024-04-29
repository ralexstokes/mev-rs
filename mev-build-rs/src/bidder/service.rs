use crate::{auctioneer::AuctionContext, bidder::strategies::DeadlineBidder};
use reth::{payload::PayloadId, primitives::U256, tasks::TaskExecutor};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::{Receiver, Sender};

pub enum Message {
    NewAuction(Arc<AuctionContext>),
}

#[derive(Debug)]
pub enum KeepAlive {
    No,
}

pub enum BidStatus {
    Dispatch(PayloadId, KeepAlive),
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    // amount in milliseconds
    pub bidding_deadline_ms: u64,
    // amount to bid as a fraction of the block's value
    // TODO: use to price bid
    pub bid_percent: Option<f64>,
    // amount to add from the builder's wallet as a subsidy to the auction bid
    // TODO: use to adjust bid
    pub subsidy_wei: Option<U256>,
}

pub struct Service {
    auctioneer: Receiver<Message>,
    bid_dispatch: Sender<BidStatus>,
    executor: TaskExecutor,
    config: Config,
}

impl Service {
    pub fn new(
        auctioneer: Receiver<Message>,
        bid_dispatch: Sender<BidStatus>,
        executor: TaskExecutor,
        config: Config,
    ) -> Self {
        Self { auctioneer, bid_dispatch, executor, config }
    }

    fn start_bid(&mut self, auction: Arc<AuctionContext>) {
        let dispatcher = self.bid_dispatch.clone();
        // TODO: make strategies configurable...
        let deadline = Duration::from_millis(self.config.bidding_deadline_ms);
        let mut strategy = DeadlineBidder::new(deadline);
        self.executor.spawn_blocking(async move {
            let bid_status = strategy.run(&auction).await;
            dispatcher.send(bid_status).await.expect("can send");
        });
    }

    async fn dispatch(&mut self, message: Message) {
        let Message::NewAuction(auction) = message;
        self.start_bid(auction);
    }

    pub async fn spawn(mut self) {
        loop {
            tokio::select! {
                Some(message) = self.auctioneer.recv() => self.dispatch(message).await,
            }
        }
    }
}
