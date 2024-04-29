use crate::{
    auctioneer::{Auctioneer, Config as AuctioneerConfig},
    bidder::{Config as BidderConfig, Service as Bidder},
    node::BuilderNode,
    payload::{
        builder_attributes::BuilderPayloadBuilderAttributes, service_builder::PayloadServiceBuilder,
    },
};
use ethereum_consensus::{
    clock::SystemClock, networks::Network, primitives::Epoch, state_transition::Context,
};
use eyre::OptionExt;
use mev_rs::{get_genesis_time, Error};
use reth::{
    api::EngineTypes,
    builder::{InitState, WithLaunchContext},
    payload::{EthBuiltPayload, PayloadBuilderHandle},
    primitives::{Address, Bytes, NamedChain},
    tasks::TaskExecutor,
};
use reth_db::DatabaseEnv;
use serde::Deserialize;
use std::{path::PathBuf, sync::Arc};
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
pub struct BuilderConfig {
    pub fee_recipient: Option<Address>,
    pub genesis_time: Option<u64>,
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
    pub bidder: Bidder,
    pub clock: SystemClock,
    pub clock_tx: Sender<ClockMessage>,
    pub context: Arc<Context>,
}

pub async fn construct<
    Engine: EngineTypes<
            PayloadBuilderAttributes = BuilderPayloadBuilderAttributes,
            BuiltPayload = EthBuiltPayload,
        > + 'static,
>(
    network: Network,
    config: Config,
    task_executor: TaskExecutor,
    payload_builder: PayloadBuilderHandle<Engine>,
) -> Result<Services<Engine>, Error> {
    let context = Arc::new(Context::try_from(network)?);

    let genesis_time = get_genesis_time(&context, config.beacon_node_url.as_ref(), None).await;

    let clock = context.clock_at(genesis_time);

    let (clock_tx, clock_rx) = broadcast::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);
    let (bidder_tx, bidder_rx) = mpsc::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);
    let (bid_dispatch_tx, bid_dispatch_rx) = mpsc::channel(DEFAULT_COMPONENT_CHANNEL_SIZE);

    let auctioneer = Auctioneer::new(
        clock_rx,
        payload_builder,
        bidder_tx,
        bid_dispatch_rx,
        config.auctioneer,
        context.clone(),
        genesis_time,
    );

    let bidder = Bidder::new(bidder_rx, bid_dispatch_tx, task_executor, config.bidder);

    Ok(Services { auctioneer, bidder, clock, clock_tx, context })
}

fn custom_network_from_config_directory(path: PathBuf) -> Network {
    let path = path.to_str().expect("is valid str").to_string();
    warn!(%path, "no named chain found; attempting to load config from custom directory");
    Network::Custom(path)
}

pub async fn launch(
    node_builder: WithLaunchContext<Arc<DatabaseEnv>, InitState>,
    custom_chain_config_directory: Option<PathBuf>,
    config: Config,
) -> eyre::Result<()> {
    let chain = node_builder.config().chain.chain;
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

    let payload_builder = PayloadServiceBuilder::try_from(&config.builder)?;

    let handle = node_builder
        .with_types(BuilderNode)
        .with_components(BuilderNode::components_with(payload_builder))
        .launch()
        .await?;

    let task_executor = handle.node.task_executor.clone();
    let payload_builder = handle.node.payload_builder.clone();
    let Services { auctioneer, bidder, clock, clock_tx, context } =
        construct(network, config, task_executor, payload_builder).await?;

    handle.node.task_executor.spawn_critical_blocking("mev-builder/auctioneer", auctioneer.spawn());
    handle.node.task_executor.spawn_critical_blocking("mev-builder/bidder", bidder.spawn());
    handle.node.task_executor.spawn_critical("mev-builder/clock", async move {
        if clock.before_genesis() {
            let duration = clock.duration_until_next_slot();
            warn!(?duration, "waiting until genesis");
            sleep(duration).await;
        }

        // TODO: block on sync here to avoid spurious first PA?

        let mut current_epoch = clock.current_epoch().expect("past genesis");
        clock_tx.send(ClockMessage::NewEpoch(current_epoch)).expect("can send");

        let mut slots = clock.into_stream();
        while let Some(slot) = slots.next().await {
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
    NewEpoch(Epoch),
}
