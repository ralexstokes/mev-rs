use crate::relay::Relay;
use beacon_api_client::Client;
use ethereum_consensus::state_transition::Context;

use mev_build_rs::{BlindedBlockProviderServer, EngineBuilder, Network};
use serde::Deserialize;
use std::{future::Future, net::Ipv4Addr, pin::Pin, sync::Arc, task::Poll};
use tokio::task::{JoinError, JoinHandle};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub beacon_node_url: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::UNSPECIFIED,
            port: 28545,
            beacon_node_url: "http://127.0.0.1:5052".into(),
        }
    }
}

pub struct Service {
    host: Ipv4Addr,
    port: u16,
    beacon_node: Client,
    context: Arc<Context>,
}

impl Service {
    pub fn from(config: Config, network: Network) -> Self {
        let endpoint: Url = config.beacon_node_url.parse().unwrap();
        let beacon_node = Client::new(endpoint);
        let context: Context = network.into();
        Self { host: config.host, port: config.port, beacon_node, context: Arc::new(context) }
    }

    /// Configures the [`Relay`] and the [`BlindedBlockProviderServer`] and spawns both to
    /// individual tasks
    pub async fn spawn(&self) -> ServiceHandle {
        let builder = EngineBuilder::new(self.context.clone());
        let relay = Relay::new(builder, self.beacon_node.clone(), self.context.clone());
        relay.initialize().await;

        let block_provider = relay.clone();
        let server = BlindedBlockProviderServer::new(self.host, self.port, block_provider).spawn();

        let relayer = tokio::spawn(async move {
            relay.run().await;
        });

        ServiceHandle { relayer, server }
    }
}

/// Contains the handles to spawned [`Relay`] and [`BlindedBlockProviderServer`] tasks
///
/// This struct is created by the [`Service::spawn`] function
#[pin_project::pin_project]
pub struct ServiceHandle {
    #[pin]
    relayer: JoinHandle<()>,
    #[pin]
    server: JoinHandle<()>,
}

impl Future for ServiceHandle {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let relayer = this.relayer.poll(cx);
        if relayer.is_ready() {
            return relayer
        }
        this.server.poll(cx)
    }
}
