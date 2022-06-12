use crate::relay::Relay;
use beacon_api_client::Client as BeaconApiClient;
use ethereum_consensus::{clock, state_transition::Context};
use futures::future::join_all;
use futures::StreamExt;
use mev_build_rs::{BlindedBlockProviderServer, EngineBuilder, EngineProxy, ProposerScheduler};
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use url::Url;

// NOTE: receivers should be fast enough relative to slot duration
// that there is never any buffered slot event but we provide some buffer
// for safety of operation
const SLOT_TIMER_CHANNEL_CAPACITY: usize = 4;

// NOTE: usually only have 1 at a time but add some small buffer for safety
const BUILD_JOB_CHANNEL_CAPACITY: usize = 4;

// NOTE: usually only have 1 at a time but add some small buffer for safety
const PROPOSER_DUTY_CHANNEL_CAPACITY: usize = 4;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub beacon_node_url: String,
    pub proxy_endpoint: String,
    pub engine_api_endpoint: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::UNSPECIFIED,
            port: 28545,
            beacon_node_url: "http://127.0.0.1:5052".into(),
            proxy_endpoint: "http://127.0.0.1:8545".into(),
            engine_api_endpoint: "http://127.0.0.1:8546".into(),
        }
    }
}

pub struct Service {
    host: Ipv4Addr,
    port: u16,
    beacon_node: BeaconApiClient,
    proxy_endpoint: Url,
    engine_api_endpoint: Url,
    context: Arc<Context>,
}

impl Service {
    pub fn from(config: Config) -> Self {
        let endpoint: Url = config.beacon_node_url.parse().unwrap();
        let beacon_node = BeaconApiClient::new(endpoint);
        let proxy_endpoint = Url::parse(&config.proxy_endpoint).unwrap();
        let engine_api_endpoint = Url::parse(&config.engine_api_endpoint).unwrap();
        let context = Arc::new(Context::for_mainnet());

        Self {
            host: config.host,
            port: config.port,
            beacon_node,
            proxy_endpoint,
            engine_api_endpoint,
            context,
        }
    }

    pub async fn run(&self) {
        // construct component graph:
        let (timer_tx, relay_timer) = broadcast::channel(SLOT_TIMER_CHANNEL_CAPACITY);
        let proposer_timer = timer_tx.subscribe();

        let (build_job_tx, build_job_rx) = mpsc::channel(BUILD_JOB_CHANNEL_CAPACITY);
        let (proposer_schedule_tx, proposer_schedule_rx) =
            mpsc::channel(PROPOSER_DUTY_CHANNEL_CAPACITY);

        let clock = clock::for_mainnet();

        let mut proposer_scheduler = ProposerScheduler::new(
            proposer_timer,
            proposer_schedule_tx,
            self.beacon_node.clone(),
            self.context.slots_per_epoch,
        );

        let engine_proxy = EngineProxy::new(
            self.proxy_endpoint.clone(),
            self.engine_api_endpoint.clone(),
        );

        let genesis_time = match self.beacon_node.get_genesis_details().await {
            Ok(details) => details.genesis_time,
            Err(err) => {
                tracing::warn!(
                    "could not get `genesis_time` from beacon node; please restart after fixing: {err}"
                );
                return;
            }
        };

        let builder = EngineBuilder::new(
            genesis_time,
            self.context.seconds_per_slot,
            self.engine_api_endpoint.clone(),
        );
        let builder_handle = builder.clone();

        let relay = Relay::new(builder, self.beacon_node.clone(), self.context.clone());

        let block_provider = relay.clone();
        let api_server = BlindedBlockProviderServer::new(self.host, self.port, block_provider);

        // initialize and launch each component:
        relay.initialize().await;
        let current_slot = clock.current_slot();
        let timer_task = tokio::spawn(async move {
            let slots = clock.stream_slots();

            tokio::pin!(slots);

            while let Some(slot) = slots.next().await {
                if let Err(err) = timer_tx.send(slot) {
                    tracing::warn!("error sending slot timer event: {err}");
                }
            }
        });

        let mut tasks = vec![timer_task];
        tasks.push(tokio::spawn(async move {
            engine_proxy.run(build_job_tx).await;
        }));
        tasks.push(tokio::spawn(async move {
            api_server.run().await;
        }));
        tasks.push(tokio::spawn(async move {
            builder_handle.run(build_job_rx, proposer_schedule_rx).await;
        }));
        tasks.push(tokio::spawn(async move {
            proposer_scheduler.run().await;
        }));
        tasks.push(tokio::spawn(async move {
            relay.run(relay_timer, current_slot).await;
        }));
        join_all(tasks).await;
    }
}
