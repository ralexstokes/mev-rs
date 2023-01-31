use crate::relay::Relay;
use beacon_api_client::Client;
use ethereum_consensus::{
    clock::{Clock, SystemTimeProvider},
    state_transition::Context,
};
use futures::StreamExt;
use mev_build_rs::EngineBuilder;
use mev_lib::{BlindedBlockProviderServer, Network};
use serde::Deserialize;
use std::{future::Future, net::Ipv4Addr, pin::Pin, sync::Arc, task::Poll};
use tokio::task::{JoinError, JoinHandle};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub beacon_node_url: String,
    #[serde(skip)]
    pub network: Network,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::UNSPECIFIED,
            port: 28545,
            beacon_node_url: "http://127.0.0.1:5052".into(),
            network: Default::default(),
        }
    }
}

pub struct Service {
    host: Ipv4Addr,
    port: u16,
    beacon_node: Client,
    network: Network,
}

impl Service {
    pub fn from(config: Config) -> Self {
        let endpoint: Url = config.beacon_node_url.parse().unwrap();
        let beacon_node = Client::new(endpoint);
        Self { host: config.host, port: config.port, beacon_node, network: config.network }
    }

    /// Configures the [`Relay`] and the [`BlindedBlockProviderServer`] and spawns both to
    /// individual tasks
    pub async fn spawn(&self) -> ServiceHandle {
        let context: Context = (&self.network).into();
        let context = Arc::new(context);
        let builder = EngineBuilder::new(context.clone());
        let relay = Relay::new(builder, self.beacon_node.clone(), context);
        relay.initialize().await;

        let block_provider = relay.clone();
        let server = BlindedBlockProviderServer::new(self.host, self.port, block_provider).spawn();

        let clock: Clock<SystemTimeProvider> = (&self.network).into();
        let relay = tokio::spawn(async move {
            let slots = clock.stream_slots();

            tokio::pin!(slots);

            let mut current_epoch = clock.current_epoch();
            let mut next_epoch = false;
            while let Some(slot) = slots.next().await {
                let epoch = clock.epoch_for(slot);
                if epoch > current_epoch {
                    current_epoch = epoch;
                    next_epoch = true;
                }
                relay.on_slot(slot, next_epoch).await;
            }
        });

        ServiceHandle { relay, server }
    }
}

/// Contains the handles to spawned [`Relay`] and [`BlindedBlockProviderServer`] tasks
///
/// This struct is created by the [`Service::spawn`] function
#[pin_project::pin_project]
pub struct ServiceHandle {
    #[pin]
    relay: JoinHandle<()>,
    #[pin]
    server: JoinHandle<()>,
}

impl Future for ServiceHandle {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let relay = this.relay.poll(cx);
        if relay.is_ready() {
            return relay
        }
        this.server.poll(cx)
    }
}
