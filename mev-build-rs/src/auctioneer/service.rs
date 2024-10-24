use crate::{
    auctioneer::auction_schedule::{AuctionSchedule, Proposals, Proposer, RelayIndex, RelaySet},
    bidder::Service as Bidder,
    compat::{
        to_blobs_bundle, to_bytes20, to_bytes32, to_execution_payload, to_execution_requests,
    },
    payload::attributes::{BuilderPayloadBuilderAttributes, ProposalAttributes},
    service::ClockMessage,
    Error,
};
use ethereum_consensus::{
    clock::convert_timestamp_to_slot,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, Slot},
    state_transition::Context,
    Fork,
};
use mev_rs::{
    relay::parse_relay_endpoints,
    signing::sign_builder_message,
    types::{block_submission, BidTrace, SignedBidSubmission},
    BlindedBlockRelayer, Relay,
};
use reth::{
    api::{BuiltPayload, EngineTypes, PayloadBuilderAttributes},
    payload::{
        EthBuiltPayload, Events, PayloadBuilder, PayloadBuilderError, PayloadBuilderHandle,
        PayloadId,
    },
};
use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::{
    broadcast,
    mpsc::{self, Receiver},
};
use tokio_stream::StreamExt;
use tracing::{debug, error, info, trace, warn};

// Fetch new proposer schedules from all connected relays at this period into the epoch
// E.g. a value of `2` corresponds to being half-way into the epoch.
const PROPOSAL_SCHEDULE_INTERVAL: u64 = 2;

const DEFAULT_BUILDER_BIDDER_CHANNEL_SIZE: usize = 16;

fn prepare_submission(
    payload: &EthBuiltPayload,
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
    let fork = context.fork_for(auction_context.slot);
    let execution_payload = to_execution_payload(payload.block(), fork)?;
    let signature = sign_builder_message(&message, signing_key, context)?;
    let submission = match fork {
        Fork::Bellatrix => {
            SignedBidSubmission::Bellatrix(block_submission::bellatrix::SignedBidSubmission {
                message,
                execution_payload,
                signature,
            })
        }
        Fork::Capella => {
            SignedBidSubmission::Capella(block_submission::capella::SignedBidSubmission {
                message,
                execution_payload,
                signature,
            })
        }
        Fork::Deneb => SignedBidSubmission::Deneb(block_submission::deneb::SignedBidSubmission {
            message,
            execution_payload,
            blobs_bundle: to_blobs_bundle(payload.sidecars())?,
            signature,
        }),
        Fork::Electra => {
            let executed_block = payload
                .executed_block()
                .ok_or_else(|| Error::PayloadBuilderError(PayloadBuilderError::MissingPayload))?;

            let execution_output = executed_block.execution_output.as_ref();
            // NOTE: assume the target requests we want are the first entry;
            let requests = execution_output.requests.first();
            let execution_requests = to_execution_requests(requests, fork)?;
            SignedBidSubmission::Electra(block_submission::electra::SignedBidSubmission {
                message,
                execution_payload,
                execution_requests,
                blobs_bundle: to_blobs_bundle(payload.sidecars())?,
                signature,
            })
        }
        fork => return Err(Error::UnsupportedFork(fork)),
    };
    Ok(submission)
}

#[derive(Debug)]
pub struct AuctionContext {
    pub slot: Slot,
    pub attributes: BuilderPayloadBuilderAttributes,
    pub proposer: Proposer,
    pub relays: RelaySet,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    /// Secret key used to sign builder messages to relay
    pub secret_key: SecretKey,
    #[serde(skip)]
    /// Public key corresponding to secret key
    pub public_key: BlsPublicKey,
    /// List of relays to submit bids
    pub relays: Vec<String>,
}

pub struct Service<
    Engine: EngineTypes<
        PayloadBuilderAttributes = BuilderPayloadBuilderAttributes,
        BuiltPayload = EthBuiltPayload,
    >,
> {
    clock: broadcast::Receiver<ClockMessage>,
    builder: PayloadBuilderHandle<Engine>,
    relays: Vec<Relay>,
    config: Config,
    context: Arc<Context>,
    // TODO consolidate this somewhere...
    genesis_time: u64,
    bidder: Bidder,
    bids: Receiver<EthBuiltPayload>,

    auction_schedule: AuctionSchedule,
    open_auctions: HashMap<PayloadId, Arc<AuctionContext>>,
    processed_payload_attributes: HashMap<Slot, HashSet<PayloadId>>,
}

impl<
        Engine: EngineTypes<
                PayloadBuilderAttributes = BuilderPayloadBuilderAttributes,
                BuiltPayload = EthBuiltPayload,
            > + 'static,
    > Service<Engine>
{
    pub fn new(
        clock: broadcast::Receiver<ClockMessage>,
        builder: PayloadBuilderHandle<Engine>,
        bidder: Bidder,
        bids: Receiver<EthBuiltPayload>,
        mut config: Config,
        context: Arc<Context>,
        genesis_time: u64,
    ) -> Self {
        let relays =
            parse_relay_endpoints(&config.relays).into_iter().map(Relay::from).collect::<Vec<_>>();

        config.public_key = config.secret_key.public_key();

        Self {
            clock,
            builder,
            relays,
            config,
            context,
            genesis_time,
            bidder,
            bids,
            auction_schedule: Default::default(),
            open_auctions: Default::default(),
            processed_payload_attributes: Default::default(),
        }
    }

    async fn fetch_proposer_schedules(&mut self) {
        // TODO: consider moving to new task on another thread, can do parallel fetch (join set)
        // and not block others at this interval
        // TODO: batch updates to auction schedule
        // TODO: consider fast data access once this stabilizes
        // TODO: rework `auction_schedule` so there is no issue with confusing relays and their
        // indices
        for (relay_index, relay) in self.relays.iter().enumerate() {
            match relay.get_proposal_schedule().await {
                Ok(schedule) => {
                    let slots = self.auction_schedule.process(relay_index, &schedule);
                    info!(?slots, %relay, "processed proposer schedule");
                }
                Err(err) => {
                    warn!(err = %err, "error fetching proposer schedule from relay")
                }
            }
        }
    }

    async fn on_slot(&mut self, slot: Slot) {
        debug!(slot, "processed");
        if (slot * PROPOSAL_SCHEDULE_INTERVAL) % self.context.slots_per_epoch == 0 {
            self.fetch_proposer_schedules().await;
        }
    }

    async fn on_epoch(&mut self, epoch: Epoch) {
        debug!(epoch, "processed");
        // NOTE: clear stale state
        let retain_slot = epoch * self.context.slots_per_epoch;
        self.auction_schedule.clear(retain_slot);
        self.open_auctions.retain(|_, auction| auction.slot >= retain_slot);
        self.processed_payload_attributes.retain(|&slot, _| slot >= retain_slot);
    }

    fn get_proposals(&self, slot: Slot) -> Option<Proposals> {
        // TODO: rework data layout to avoid expensive clone
        self.auction_schedule.get_matching_proposals(slot).cloned()
    }

    fn store_auction(&mut self, auction: AuctionContext) -> Arc<AuctionContext> {
        let payload_id = auction.attributes.payload_id();
        // TODO: consider data layout in `open_auctions`
        self.open_auctions.entry(payload_id).or_insert_with(|| Arc::new(auction)).clone()
    }

    async fn open_auction(
        &mut self,
        slot: Slot,
        proposer: Proposer,
        relays: HashSet<RelayIndex>,
        mut attributes: BuilderPayloadBuilderAttributes,
    ) -> Option<PayloadId> {
        let (bidder, revenue_updates) = mpsc::channel(DEFAULT_BUILDER_BIDDER_CHANNEL_SIZE);
        let proposal = ProposalAttributes {
            proposer_gas_limit: proposer.gas_limit,
            proposer_fee_recipient: proposer.fee_recipient,
            bidder,
        };
        attributes.attach_proposal(proposal);

        // TODO: can likely skip full attributes in `AuctionContext`
        // TODO: consider data layout here...
        // TODO: can likely refactor around auction schedule to skip some clones...
        let auction = AuctionContext { slot, attributes, proposer, relays };

        // TODO: work out cancellation discipline
        let auction = self.store_auction(auction);

        if let Err(err) = self.builder.send_new_payload(auction.attributes.clone()).await {
            warn!(%err, "could not start build with payload builder");
            return None
        }

        let payload_id = auction.attributes.payload_id();
        self.bidder.start_bid(auction, revenue_updates);
        Some(payload_id)
    }

    // Record `payload_id` as processed so that we can identify duplicate notifications.
    // Return value indicates if the `payload_id` has been observed before or not.
    fn observe_payload_id(&mut self, slot: Slot, payload_id: PayloadId) -> bool {
        let processed_set = self.processed_payload_attributes.entry(slot).or_default();
        processed_set.insert(payload_id)
    }

    async fn on_payload_attributes(&mut self, attributes: BuilderPayloadBuilderAttributes) {
        let slot = convert_timestamp_to_slot(
            attributes.timestamp(),
            self.genesis_time,
            self.context.seconds_per_slot,
        )
        .expect("is past genesis");

        let is_new = self.observe_payload_id(slot, attributes.payload_id());

        if !is_new {
            trace!(payload_id = %attributes.payload_id(), "ignoring duplicate payload attributes");
            return
        }

        if let Some(proposals) = self.get_proposals(slot) {
            for (proposer, relays) in proposals {
                if let Some(payload_id) =
                    self.open_auction(slot, proposer, relays, attributes.clone()).await
                {
                    self.observe_payload_id(slot, payload_id);
                }
            }
        }
    }

    async fn submit_payload(&self, payload: EthBuiltPayload) {
        // TODO: resolve hot fix for short slot timings
        let auction = self.open_auctions.get(&payload.id());
        if auction.is_none() {
            return
        }
        let auction = auction.unwrap();
        let mut successful_relays_for_submission = Vec::with_capacity(auction.relays.len());
        match prepare_submission(
            &payload,
            &self.config.secret_key,
            &self.config.public_key,
            auction,
            &self.context,
        ) {
            Ok(signed_submission) => {
                // TODO: parallel dispatch
                for &relay_index in &auction.relays {
                    match self.relays.get(relay_index) {
                        Some(relay) => {
                            if let Err(err) = relay.submit_bid(&signed_submission).await {
                                warn!(%err, ?relay, slot = auction.slot, "could not submit payload");
                            } else {
                                successful_relays_for_submission.push(relay_index);
                            }
                        }
                        None => {
                            // NOTE: this arm signals a violation of an internal invariant
                            // Please fix if you see this error
                            error!(relay_index, "could not dispatch to unknown relay");
                        }
                    }
                }
            }
            Err(err) => {
                warn!(%err, slot = auction.slot, "could not prepare submission")
            }
        }
        if !successful_relays_for_submission.is_empty() {
            let relay_set = successful_relays_for_submission
                .into_iter()
                .map(|index| format!("{0}", self.relays[index]))
                .collect::<Vec<_>>();
            info!(
                slot = auction.slot,
                block_number = payload.block().number,
                block_hash = %payload.block().hash(),
                parent_hash = %payload.block().header.header().parent_hash,
                txn_count = %payload.block().body.transactions.len(),
                blob_count = %payload.sidecars().iter().map(|s| s.blobs.len()).sum::<usize>(),
                value = %payload.fees(),
                relays=?relay_set,
                "payload submitted"
            );
        }
    }

    async fn process_clock(&mut self, message: ClockMessage) {
        use ClockMessage::*;
        match message {
            NewSlot(slot) => self.on_slot(slot).await,
            NewEpoch(epoch) => self.on_epoch(epoch).await,
        }
    }

    async fn process_payload_event(&mut self, event: Events<Engine>) {
        if let Events::Attributes(attributes) = event {
            self.on_payload_attributes(attributes).await;
        }
    }

    pub async fn spawn(mut self) {
        if self.relays.is_empty() {
            warn!("no valid relays provided in config");
        } else {
            let count = self.relays.len();
            info!(count, relays = ?self.relays, "configured with relay(s)");
        }

        // initialize proposer schedule
        self.fetch_proposer_schedules().await;

        let mut payload_events =
            self.builder.subscribe().await.expect("can subscribe to events").into_stream();

        loop {
            tokio::select! {
                Ok(message) = self.clock.recv() => self.process_clock(message).await,
                Some(event) = payload_events.next() => match event {
                    Ok(event) =>  self.process_payload_event(event).await,
                    Err(err) => warn!(%err, "error getting payload event"),
                },
                Some(payload) = self.bids.recv() => self.submit_payload(payload).await,
            }
        }
    }
}
