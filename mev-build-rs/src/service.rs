use crate::{
    auctioneer::{Config as AuctioneerConfig, Service as Auctioneer},
    bidder::{Config as BidderConfig, Service as Bidder},
    node::BuilderNode,
    payload::{
        attributes::BuilderPayloadBuilderAttributes, service_builder::PayloadServiceBuilder,
    },
};
use ethereum_consensus::{
    clock::SystemClock,
    networks::Network,
    primitives::{Epoch, Slot},
    state_transition::Context,
};
use eyre::OptionExt;
use mev_rs::{get_genesis_time, Error};
use reth::{
    api::EngineTypes,
    builder::{NodeBuilder, WithLaunchContext},
    payload::{EthBuiltPayload, PayloadBuilderHandle},
    primitives::{Address, Bytes, NamedChain},
    tasks::TaskExecutor,
};
use reth_db::DatabaseEnv;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::{
    broadcast::{self, Sender},
    mpsc,
};
use tokio_stream::StreamExt;
use tracing::warn;

pub const DEFAULT_COMPONENT_CHANNEL_SIZE: usize = 16;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct BuilderConfig {
    pub fee_recipient: Option<Address>,
    pub extra_data: Option<Bytes>,
    pub execution_mnemonic: String,
}

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    pub auctioneer: AuctioneerConfig,
    pub builder: BuilderConfig,
    pub bidder: BidderConfig,

    // Used to get genesis time, if one can't be found without a network call
    pub beacon_node_url: Option<String>,
}

pub struct Services<
    Engine: EngineTypes<
        PayloadBuilderAttributes = BuilderPayloadBuilderAttributes,
        BuiltPayload = EthBuiltPayload,
    >,
> {
    pub auctioneer: Auctioneer<Engine>,
    pub clock: SystemClock,
    pub clock_tx: Sender<ClockMessage>,
}

pub async fn construct_services<
    Engine: EngineTypes<
            PayloadBuilderAttributes = BuilderPayloadBuilderAttributes,
            BuiltPayload = EthBuiltPayload,
        > + 'static,
>(
    network: Network,
    config: Config,
    task_executor: TaskExecutor,
    payload_builder: PayloadBuilderHandle<Engine>,
    bid_rx: mpsc::Receiver<EthBuiltPayload>,
) -> Result<Services<Engine>, Error> {
    let context = Arc::new(Context::try_from(network)?);

    let genesis_time = get_genesis_time(&context, config.beacon_node_url.as_ref(), None).await;

    let clock = context.clock_at(genesis_time);

    let (clock_tx, clock_rx) = broadcast::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);

    let bidder = Bidder::new(task_executor, config.bidder);
    let auctioneer = Auctioneer::new(
        clock_rx,
        payload_builder,
        bidder,
        bid_rx,
        config.auctioneer,
        context,
        genesis_time,
    );

    Ok(Services { auctioneer, clock, clock_tx })
}

fn custom_network_from_config_directory(path: PathBuf) -> Network {
    let path = path.to_str().expect("is valid str").to_string();
    warn!(%path, "no named chain found; attempting to load config from custom directory");
    Network::Custom(path)
}

pub async fn launch(
    node_builder: WithLaunchContext<NodeBuilder<Arc<DatabaseEnv>>>,
    custom_chain_config_directory: Option<PathBuf>,
    config: Config,
) -> eyre::Result<()> {
    let (bid_tx, bid_rx) = mpsc::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);
    let payload_builder = PayloadServiceBuilder::try_from((&config.builder, bid_tx))?;

    let handle = node_builder
        .with_types::<BuilderNode>()
        .with_components(BuilderNode::components_with(payload_builder))
        .launch()
        .await?;

    let chain = handle.node.config.chain.chain;
    let network = if let Some(chain) = chain.named() {
        match chain {
            NamedChain::Mainnet => Network::Mainnet,
            NamedChain::Sepolia => Network::Sepolia,
            NamedChain::Holesky => Network::Holesky,
            _ => {
                let path = custom_chain_config_directory
                    .ok_or_eyre("missing custom chain configuration when expected")?;
                custom_network_from_config_directory(path)
            }
        }
    } else {
        let path = custom_chain_config_directory
            .ok_or_eyre("missing custom chain configuration when expected")?;
        custom_network_from_config_directory(path)
    };

    let task_executor = handle.node.task_executor.clone();
    let payload_builder = handle.node.payload_builder.clone();
    let Services { auctioneer, clock, clock_tx } =
        construct_services(network, config, task_executor, payload_builder, bid_rx).await?;

    handle.node.task_executor.spawn_critical_blocking("mev-builder/auctioneer", auctioneer.spawn());
    handle.node.task_executor.spawn_critical("mev-builder/clock", async move {
        let mut slots = clock.clone().into_stream();

        // NOTE: this will block until genesis if we are before the genesis time
        let current_slot = slots.next().await.expect("some next slot");
        let mut current_epoch = clock.epoch_for(current_slot);

        // TODO: block on sync here to avoid spurious first PA?

        if let Err(err) = clock_tx.send(ClockMessage::NewSlot(current_slot)) {
            let msg = err.0;
            warn!(?msg, "could not update receivers with new slot")
        }
        if let Err(err) = clock_tx.send(ClockMessage::NewEpoch(current_epoch)) {
            let msg = err.0;
            warn!(?msg, "could not update receivers with new epoch");
        }

        while let Some(slot) = slots.next().await {
            if let Err(err) = clock_tx.send(ClockMessage::NewSlot(slot)) {
                let msg = err.0;
                warn!(?msg, "could not update receivers with new slot")
            }
            let epoch = clock.epoch_for(slot);
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
    NewSlot(Slot),
    NewEpoch(Epoch),
}
