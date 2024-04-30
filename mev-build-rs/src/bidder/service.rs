use crate::{
    auctioneer::AuctionContext,
    bidder::{strategies::BasicStrategy, Bid, Config, KeepAlive},
    Error,
};
use ethereum_consensus::clock::duration_until;
use reth::{
    api::PayloadBuilderAttributes, payload::PayloadId, primitives::U256, tasks::TaskExecutor,
};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{
        mpsc::{Receiver, Sender},
        oneshot,
    },
    time::{sleep, timeout},
};
use tracing::debug;

/// All bidding routines stop this many seconds *after* the timestamp of the proposal
/// regardless of what the bidding strategy suggests
pub const DEFAULT_BIDDING_DEADLINE_AFTER_SLOT: u64 = 1;

pub enum Message {
    NewAuction(Arc<AuctionContext>),
    Dispatch { payload_id: PayloadId, value: U256, keep_alive: KeepAlive },
    RevenueQuery(PayloadId, oneshot::Sender<Result<U256, Error>>),
}

pub struct Service {
    auctioneer_rx: Receiver<Message>,
    auctioneer_tx: Sender<Message>,
    executor: TaskExecutor,
    config: Config,
}

impl Service {
    pub fn new(
        auctioneer_rx: Receiver<Message>,
        auctioneer_tx: Sender<Message>,
        executor: TaskExecutor,
        config: Config,
    ) -> Self {
        Self { auctioneer_rx, auctioneer_tx, executor, config }
    }

    fn start_bid(&mut self, auction: Arc<AuctionContext>) {
        let auctioneer = self.auctioneer_tx.clone();
        // TODO: make strategies configurable...
        let mut strategy = BasicStrategy::new(&self.config);
        let duration_after_slot = Duration::from_secs(DEFAULT_BIDDING_DEADLINE_AFTER_SLOT);
        let max_bidding_duration = duration_until(auction.attributes.timestamp())
            .checked_add(duration_after_slot)
            .unwrap_or_default();
        self.executor.spawn_blocking(async move {
            // TODO issues with timeout and open channels?
            let _ = timeout(max_bidding_duration, async move {
                let payload_id = auction.attributes.payload_id();
                let mut should_run = KeepAlive::Yes;
                while matches!(should_run, KeepAlive::Yes) {
                    // TODO: payload builder should stream (payload_id, block_hash, fees) for each
                    // constructed block
                    let (tx, rx) = oneshot::channel();
                    let message = Message::RevenueQuery(payload_id, tx);
                    auctioneer.send(message).await.expect("can send");
                    let current_revenue = match rx.await.expect("can recv") {
                        Ok(fees) => fees,
                        Err(err) => {
                            // NOTE: if there was an error, try to fetch
                            // again without running a strategy
                            // TODO: handle case when the auction has terminated and we should
                            // also terminate
                            debug!(%err, "could not get current revenue; trying again");
                            continue
                        }
                    };

                    match strategy.run(&auction, current_revenue).await {
                        Bid::Wait(duration) => {
                            sleep(duration).await;
                            continue
                        }
                        Bid::Submit { value, keep_alive } => {
                            should_run = keep_alive;
                            auctioneer
                                .send(Message::Dispatch { payload_id, value, keep_alive })
                                .await
                                .expect("can send");
                        }
                    }
                }
            })
            .await;
        });
    }

    async fn dispatch(&mut self, message: Message) {
        if let Message::NewAuction(auction) = message {
            self.start_bid(auction);
        }
    }

    pub async fn spawn(mut self) {
        loop {
            tokio::select! {
                Some(message) = self.auctioneer_rx.recv() => self.dispatch(message).await,
            }
        }
    }
}
