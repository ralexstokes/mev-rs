use crate::{
    auction_schedule::{AuctionSchedule, Proposals},
    bidder::{AuctionContext, BidRequest, DeadlineBidder},
    builder::{KeepAlive, Message as BuilderMessage},
    service::ClockMessage,
    utils::compat::{to_bytes20, to_bytes32, to_execution_payload},
    Error,
};
use ethereum_consensus::{
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, Slot},
    state_transition::Context,
};
use mev_rs::{
    relay::parse_relay_endpoints,
    signing::sign_builder_message,
    types::{BidTrace, SignedBidSubmission},
    BlindedBlockRelayer, Relay,
};
use reth::{
    api::PayloadBuilderAttributes,
    payload::{EthBuiltPayload, PayloadId},
    tasks::TaskExecutor,
};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{
    broadcast,
    mpsc::{Receiver, Sender},
    oneshot,
};
use tracing::{info, warn};

fn prepare_submission(
    payload: EthBuiltPayload,
    signing_key: &SecretKey,
    public_key: &BlsPublicKey,
    auction_context: &AuctionContext,
    context: &Context,
) -> Result<SignedBidSubmission, Error> {
    let message = BidTrace {
        slot: auction_context.slot,
        parent_hash: to_bytes32(auction_context.attributes.inner.parent),
        block_hash: to_bytes32(payload.block().hash()),
        builder_public_key: public_key.clone(),
        proposer_public_key: auction_context.proposer.public_key.clone(),
        proposer_fee_recipient: to_bytes20(auction_context.proposer.fee_recipient),
        gas_limit: payload.block().gas_limit,
        gas_used: payload.block().gas_used,
        value: payload.fees(),
    };
    let execution_payload = to_execution_payload(payload.block());
    let signature = sign_builder_message(&message, signing_key, context)?;
    Ok(SignedBidSubmission { message, execution_payload, signature })
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    pub secret_key: SecretKey,
    #[serde(skip)]
    pub public_key: BlsPublicKey,
    pub relays: Vec<String>,
}

pub enum Message {
    ProposalQuery(Slot, oneshot::Sender<Option<Proposals>>),
    // TODO: can likely scope to just payload ids, as long as they are linked to corresponding
    // proposals and keep `AuctionContext` local to here
    NewAuctions(Vec<AuctionContext>),
    BuiltPayload(EthBuiltPayload),
}

pub struct Auctioneer {
    msgs: Receiver<Message>,
    clock: broadcast::Receiver<ClockMessage>,
    builder: Sender<BuilderMessage>,
    relays: Vec<Arc<Relay>>,
    auction_schedule: AuctionSchedule,
    open_auctions: HashMap<PayloadId, Arc<AuctionContext>>,
    executor: TaskExecutor,
    config: Config,
    context: Arc<Context>,
}

impl Auctioneer {
    pub fn new(
        msgs: Receiver<Message>,
        clock: broadcast::Receiver<ClockMessage>,
        builder: Sender<BuilderMessage>,
        executor: TaskExecutor,
        mut config: Config,
        context: Arc<Context>,
    ) -> Self {
        let relays = parse_relay_endpoints(&config.relays)
            .into_iter()
            .map(|endpoint| Arc::new(Relay::from(endpoint)))
            .collect::<Vec<_>>();

        config.public_key = config.secret_key.public_key();
        Self {
            msgs,
            clock,
            builder,
            relays,
            auction_schedule: Default::default(),
            open_auctions: Default::default(),
            executor,
            config,
            context,
        }
    }

    fn take_proposals(&mut self, slot: Slot) -> Option<Proposals> {
        self.auction_schedule.take_matching_proposals(slot)
    }

    fn process_new_auction(&mut self, auction: AuctionContext) {
        let payload_id = auction.attributes.payload_id();
        self.open_auctions.insert(payload_id, Arc::new(auction));
        let auction = self.open_auctions.get(&payload_id).unwrap().clone();

        let builder = self.builder.clone();
        // TODO refactor into independent actor
        // this works for now, but want bidding to happen on separate thread
        self.executor.spawn_blocking(async move {
            let deadline = Duration::from_secs(1);
            let bidder = DeadlineBidder::new(deadline);
            match bidder.make_bid(&auction).await {
                BidRequest::Ready(payload_id) => {
                    builder
                        .send(BuilderMessage::FetchPayload(payload_id, KeepAlive::No))
                        .await
                        .expect("can send");
                }
            }
        });
    }

    fn process_new_auctions(&mut self, auctions: Vec<AuctionContext>) {
        for auction in auctions {
            self.process_new_auction(auction);
        }
    }

    async fn on_epoch(&mut self, epoch: Epoch) {
        // TODO: concurrent fetch
        // TODO: batch updates to auction schedule
        for relay in self.relays.iter() {
            match relay.get_proposal_schedule().await {
                Ok(schedule) => {
                    let slots = self.auction_schedule.process(relay.clone(), &schedule);
                    info!(epoch, ?slots, %relay, "processed proposer schedule");
                }
                Err(err) => {
                    warn!(err = %err, "error fetching proposer schedule from relay")
                }
            }
        }

        let slot = epoch * self.context.slots_per_epoch;
        self.auction_schedule.clear(slot);

        self.open_auctions.retain(|_, auction| auction.slot >= slot);
    }

    async fn submit_payload(&self, payload: EthBuiltPayload) {
        let auction = self.open_auctions.get(&payload.id()).expect("has auction");
        // TODO: should convert to ExecutionPayloadV3 etc. for blobs etc.
        match prepare_submission(
            payload,
            &self.config.secret_key,
            &self.config.public_key,
            auction,
            &self.context,
        ) {
            Ok(signed_submission) => {
                let relays = &auction.relays;
                for relay in relays {
                    if let Err(err) = relay.submit_bid(&signed_submission).await {
                        warn!(%err, ?relay, slot = auction.slot, "could not submit payload");
                    }
                }
            }
            Err(err) => {
                warn!(%err, slot = auction.slot, "could not prepare submission")
            }
        }
    }

    async fn dispatch(&mut self, message: Message) {
        use Message::*;
        match message {
            ProposalQuery(slot, tx) => {
                let proposals = self.take_proposals(slot);
                tx.send(proposals).expect("can send");
            }
            NewAuctions(auctions) => self.process_new_auctions(auctions),
            BuiltPayload(payload) => self.submit_payload(payload).await,
        }
    }

    async fn dispatch_clock(&mut self, message: ClockMessage) {
        if let ClockMessage::NewEpoch(epoch) = message {
            self.on_epoch(epoch).await;
        }
    }

    pub async fn spawn(mut self) {
        if self.relays.is_empty() {
            warn!("no valid relays provided in config");
        } else {
            let count = self.relays.len();
            info!("configured with {count} relay(s)");
            for relay in self.relays.iter() {
                info!(%relay, "configured with relay");
            }
        }

        loop {
            tokio::select! {
                Some(message) = self.msgs.recv() => self.dispatch(message).await,
                Ok(message) = self.clock.recv() => self.dispatch_clock(message).await,
            }
        }
    }
}
