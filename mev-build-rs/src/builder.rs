use crate::{
    auction_schedule::{Proposals, Proposer},
    auctioneer::Message as AuctioneerMessage,
    bidder::AuctionContext,
    Error,
};
use ethereum_consensus::{
    clock::convert_timestamp_to_slot, primitives::Slot, state_transition::Context,
};
use reth::{
    api::{EngineTypes, PayloadBuilderAttributes},
    payload::{
        EthBuiltPayload, EthPayloadBuilderAttributes, Events, PayloadBuilderHandle, PayloadId,
        PayloadStore,
    },
    primitives::Address,
    rpc::{
        compat::engine::convert_withdrawal_to_standalone_withdraw, types::engine::PayloadAttributes,
    },
    tasks::TaskExecutor,
};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::{
    mpsc::{Receiver, Sender},
    oneshot,
};
use tokio_stream::StreamExt;
use tracing::warn;

fn make_attributes_for_proposer(
    attributes: &EthPayloadBuilderAttributes,
    builder_fee_recipient: Address,
) -> EthPayloadBuilderAttributes {
    // TODO: extend attributes with gas limit and proposer fee recipient
    let withdrawals = if attributes.withdrawals.is_empty() {
        None
    } else {
        Some(
            attributes
                .withdrawals
                .iter()
                .cloned()
                .map(convert_withdrawal_to_standalone_withdraw)
                .collect(),
        )
    };
    let payload_attributes = PayloadAttributes {
        timestamp: attributes.timestamp,
        prev_randao: attributes.prev_randao,
        suggested_fee_recipient: builder_fee_recipient,
        withdrawals,
        parent_beacon_block_root: attributes.parent_beacon_block_root,
    };

    EthPayloadBuilderAttributes::try_new(attributes.parent, payload_attributes)
        .expect("conversion currently always succeeds")
}

pub enum KeepAlive {
    No,
}

pub enum Message {
    FetchPayload(PayloadId, KeepAlive),
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    pub fee_recipient: Address,
    pub genesis_time: Option<u64>,
}

pub struct Builder<
    Engine: EngineTypes<
        PayloadBuilderAttributes = EthPayloadBuilderAttributes,
        BuiltPayload = EthBuiltPayload,
    >,
> {
    msgs: Receiver<Message>,
    auctioneer: Sender<AuctioneerMessage>,
    payload_builder: PayloadBuilderHandle<Engine>,
    payload_store: PayloadStore<Engine>,
    executor: TaskExecutor,
    config: Config,
    context: Arc<Context>,
    genesis_time: u64,
}

impl<
        Engine: EngineTypes<
                PayloadBuilderAttributes = EthPayloadBuilderAttributes,
                BuiltPayload = EthBuiltPayload,
            > + 'static,
    > Builder<Engine>
{
    pub fn new(
        msgs: Receiver<Message>,
        auctioneer: Sender<AuctioneerMessage>,
        payload_builder: PayloadBuilderHandle<Engine>,
        executor: TaskExecutor,
        config: Config,
        context: Arc<Context>,
        genesis_time: u64,
    ) -> Self {
        let payload_store = payload_builder.clone().into();
        Self {
            msgs,
            auctioneer,
            payload_builder,
            payload_store,
            executor,
            config,
            context,
            genesis_time,
        }
    }

    pub async fn process_proposals(
        &self,
        slot: Slot,
        attributes: EthPayloadBuilderAttributes,
        proposals: Option<Proposals>,
    ) -> Result<Vec<AuctionContext>, Error> {
        let mut new_auctions = vec![];

        if let Some(proposals) = proposals {
            for (proposer, relays) in proposals {
                let attributes =
                    make_attributes_for_proposer(&attributes, self.config.fee_recipient);

                if let Some(attributes) = self.start_build(&proposer, &attributes).await {
                    // TODO: can likely skip full attributes in `AuctionContext`, can skip clone in
                    // `start_build`
                    let auction = AuctionContext { slot, attributes, proposer, relays };
                    new_auctions.push(auction);
                }
            }
        }
        Ok(new_auctions)
    }

    // TODO: can likely skip returning attributes here...
    async fn start_build(
        &self,
        _proposer: &Proposer,
        attributes: &EthPayloadBuilderAttributes,
    ) -> Option<EthPayloadBuilderAttributes> {
        match self.payload_builder.new_payload(attributes.clone()).await {
            Ok(payload_id) => {
                let attributes_payload_id = attributes.payload_id();
                if payload_id == attributes_payload_id {
                    Some(attributes.clone())
                } else {
                    warn!(%payload_id, %attributes_payload_id, "mismatch between computed payload id and the one returned by the payload builder");
                    None
                }
            }
            Err(err) => {
                warn!(%err, "bulder could not start build with payload builder");
                None
            }
        }
    }

    fn terminate_job(&self, payload_id: PayloadId) {
        let payload_store = self.payload_store.clone();
        self.executor.spawn(async move {
            // NOTE: terminate job and discard any built payload
            let _ = payload_store.resolve(payload_id).await;
        });
    }

    async fn on_payload_attributes(&self, attributes: EthPayloadBuilderAttributes) {
        // NOTE: the payload builder currently makes a job for the incoming `attributes`.
        // We want to customize the building logic and so we cancel this first job unconditionally.
        self.terminate_job(attributes.payload_id());

        // TODO: move slot calc to auctioneer?
        let slot = convert_timestamp_to_slot(
            attributes.timestamp,
            self.genesis_time,
            self.context.seconds_per_slot,
        )
        .expect("is past genesis");
        let (tx, rx) = oneshot::channel();
        self.auctioneer.send(AuctioneerMessage::ProposalQuery(slot, tx)).await.expect("can send");
        let proposals = rx.await.expect("can recv");
        let auctions = self.process_proposals(slot, attributes, proposals).await;
        match auctions {
            Ok(auctions) => {
                self.auctioneer
                    .send(AuctioneerMessage::NewAuctions(auctions))
                    .await
                    .expect("can send");
            }
            Err(err) => {
                warn!(%err, "could not send new auctions to auctioneer");
            }
        }
    }

    async fn send_payload_to_auctioneer(&self, payload_id: PayloadId, _keep_alive: KeepAlive) {
        // TODO: use signal from bidder to know if we should keep refining a given payload, or can
        // extract the final build
        match self.payload_store.best_payload(payload_id).await.expect("exists") {
            Ok(payload) => self
                .auctioneer
                .send(AuctioneerMessage::BuiltPayload(payload))
                .await
                .expect("can send"),
            Err(err) => {
                warn!(%err, "could not get payload when requested")
            }
        }
    }

    async fn dispatch(&self, message: Message) {
        match message {
            Message::FetchPayload(payload_id, keep_alive) => {
                self.send_payload_to_auctioneer(payload_id, keep_alive).await;
            }
        }
    }

    pub async fn spawn(mut self) {
        let mut payload_events =
            self.payload_builder.subscribe().await.expect("can subscribe to events").into_stream();
        loop {
            tokio::select! {
                Some(message) = self.msgs.recv() => self.dispatch(message).await,
                Some(Ok(Events::Attributes(attributes))) = payload_events.next() => self.on_payload_attributes(attributes).await,
            }
        }
    }
}
