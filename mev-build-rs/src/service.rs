use crate::{
    auctioneer::{Auctioneer, Config as AuctioneerConfig},
    builder::{Builder, Config as BuilderConfig},
    payload::service_builder::PayloadServiceBuilder,
};
use ethereum_consensus::{
    clock::SystemClock, networks::Network, primitives::Epoch, state_transition::Context,
};
use mev_rs::{get_genesis_time, Error};
use reth::{
    api::EngineTypes,
    builder::{InitState, WithLaunchContext},
    payload::{EthBuiltPayload, EthPayloadBuilderAttributes, PayloadBuilderHandle},
    primitives::Bytes,
    tasks::TaskExecutor,
};
use reth_db::DatabaseEnv;
use reth_node_ethereum::EthereumNode;
use serde::Deserialize;
use std::sync::Arc;
use tokio::{
    sync::{
        broadcast::{self, Sender},
        mpsc,
    },
    time::sleep,
};
use tokio_stream::StreamExt;
use tracing::warn;

pub const DEFAULT_COMPONENT_CHANNEL_SIZE: usize = 16;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    // TODO: move to payload builder
    pub extra_data: Bytes,
    // TODO: move to payload builder
    pub execution_mnemonic: String,
    // TODO: move to bidder
    // amount in milliseconds
    pub bidding_deadline_ms: u64,
    // TODO: move to payload builder, or have as part of bidder config
    // amount to bid as a fraction of the block's value
    pub bid_percent: Option<f64>,
    // TODO: move to bidder
    // amount to add from the builder's wallet as a subsidy to the auction bid
    pub subsidy_gwei: Option<u64>,

    pub auctioneer: AuctioneerConfig,
    pub builder: BuilderConfig,

    // Used to get genesis time, if one can't be found without a network call
    pub beacon_node_url: Option<String>,
}

pub struct Services<
    Engine: EngineTypes<
        PayloadBuilderAttributes = EthPayloadBuilderAttributes,
        BuiltPayload = EthBuiltPayload,
    >,
> {
    pub auctioneer: Auctioneer,
    pub builder: Builder<Engine>,
    pub clock: SystemClock,
    pub clock_tx: Sender<ClockMessage>,
    pub context: Arc<Context>,
}

pub async fn construct<
    Engine: EngineTypes<
            PayloadBuilderAttributes = EthPayloadBuilderAttributes,
            BuiltPayload = EthBuiltPayload,
        > + 'static,
>(
    network: Network,
    config: Config,
    task_executor: TaskExecutor,
    payload_builder: PayloadBuilderHandle<Engine>,
) -> Result<Services<Engine>, Error> {
    let (clock_tx, clock_rx) = broadcast::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);
    let context = Arc::new(Context::try_from(network)?);

    let genesis_time = get_genesis_time(&context, config.beacon_node_url.as_ref(), None).await;

    let clock = context.clock_at(genesis_time);

    let (builder_tx, builder_rx) = mpsc::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);
    let (auctioneer_tx, auctioneer_rx) = mpsc::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);

    let builder = Builder::new(
        builder_rx,
        auctioneer_tx,
        payload_builder,
        task_executor.clone(),
        config.builder,
        context.clone(),
        genesis_time,
    );

    let auctioneer = Auctioneer::new(
        auctioneer_rx,
        clock_rx,
        builder_tx,
        task_executor,
        config.auctioneer,
        context.clone(),
    );

    Ok(Services { auctioneer, builder, clock, clock_tx, context })
}

pub async fn launch(
    node_builder: WithLaunchContext<Arc<DatabaseEnv>, InitState>,
    network: Network,
    config: Config,
) -> eyre::Result<()> {
    let chain_spec = &node_builder.config().chain;
    let chain_name = chain_spec.chain.to_string();
    if chain_name != network.to_string() {
        return Err(eyre::eyre!("configuration file did not match CLI"))
    }

    // TODO:  ability to just run reth

    let handle = node_builder
        .with_types(EthereumNode::default())
        .with_components(EthereumNode::components().payload(PayloadServiceBuilder))
        .launch()
        .await?;

    let task_executor = handle.node.task_executor.clone();
    let payload_builder = handle.node.payload_builder.clone();
    let Services { auctioneer, builder, clock, clock_tx, context } =
        construct(network, config, task_executor, payload_builder).await?;

    handle.node.task_executor.spawn_critical("mev-builder/auctioneer", auctioneer.spawn());
    handle.node.task_executor.spawn_critical("mev-builder/builder", builder.spawn());
    handle.node.task_executor.spawn_critical("mev-builder/clock", async move {
        if clock.before_genesis() {
            let duration = clock.duration_until_next_slot();
            warn!(?duration, "scheduler waiting until genesis");
            sleep(duration).await;
        }

        let mut current_epoch = clock.current_epoch().expect("past genesis");
        clock_tx.send(ClockMessage::NewEpoch(current_epoch)).expect("can send");

        let mut slots = clock.into_stream();
        while let Some(slot) = slots.next().await {
            if let Err(err) = clock_tx.send(ClockMessage::NewSlot) {
                let msg = err.0;
                warn!(?msg, "could not update receivers with new slot")
            }
            let epoch = slot / context.slots_per_epoch;
            if epoch > current_epoch {
                current_epoch = epoch;
                if let Err(err) = clock_tx.send(ClockMessage::NewEpoch(epoch)) {
                    let msg = err.0;
                    warn!(?msg, "could not update receivers with new epoch")
                }
            }
        }
    });

    handle.wait_for_node_exit().await
}

#[derive(Debug, Clone)]
pub enum ClockMessage {
    NewSlot,
    NewEpoch(Epoch),
}
