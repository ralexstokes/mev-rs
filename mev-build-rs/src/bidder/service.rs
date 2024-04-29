use crate::{
    auctioneer::AuctionContext,
    bidder::{strategies::DeadlineBidder, Config},
};
use reth::{
    api::PayloadBuilderAttributes, payload::PayloadId, primitives::U256, tasks::TaskExecutor,
};
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};

pub enum Message {
    NewAuction(Arc<AuctionContext>),
    Dispatch(PayloadId, KeepAlive),
}

#[derive(Debug)]
pub enum KeepAlive {
    No,
}

pub enum BidStatus {
    Submit { value: U256, keep_alive: KeepAlive },
}

pub struct Service {
    auctioneer: Receiver<Message>,
    bid_dispatch: Sender<Message>,
    executor: TaskExecutor,
    config: Config,
}

impl Service {
    pub fn new(
        auctioneer: Receiver<Message>,
        bid_dispatch: Sender<Message>,
        executor: TaskExecutor,
        config: Config,
    ) -> Self {
        Self { auctioneer, bid_dispatch, executor, config }
    }

    fn start_bid(&mut self, auction: Arc<AuctionContext>) {
        let dispatcher = self.bid_dispatch.clone();
        // TODO: make strategies configurable...
        let mut strategy = DeadlineBidder::new(&self.config);
        self.executor.spawn_blocking(async move {
            // TODO get current fees from builder
            let fees = U256::from(100);
            let BidStatus::Submit { value: _value, keep_alive } =
                strategy.run(&auction, fees).await;
            // TODO send value to builder

            dispatcher
                .send(Message::Dispatch(auction.attributes.payload_id(), keep_alive))
                .await
                .expect("can send");
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
                Some(message) = self.auctioneer.recv() => self.dispatch(message).await,
            }
        }
    }
}
