use crate::relay_mux::RelayMux;
use ethereum_consensus::{networks, networks::Network, state_transition::Context};
use futures::StreamExt;
use mev_rs::{
    blinded_block_provider::Server as BlindedBlockProviderServer,
    relay::{Relay, RelayEndpoint},
    Error,
};
use serde::Deserialize;
use std::{future::Future, net::Ipv4Addr, pin::Pin, task::Poll};
use tokio::task::{JoinError, JoinHandle};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub relays: Vec<String>,
    #[serde(default)]
    pub network: Network,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::UNSPECIFIED,
            port: 18550,
            relays: vec![],
            network: Network::default(),
        }
    }
}

fn parse_relay_endpoints(relay_urls: &[String]) -> Vec<RelayEndpoint> {
    let mut relays = vec![];

    for relay_url in relay_urls {
        match relay_url.parse::<Url>() {
            Ok(url) => match RelayEndpoint::try_from(url) {
                Ok(relay) => relays.push(relay),
                Err(err) => tracing::warn!("error parsing relay from URL `{relay_url}`: {err}"),
            },
            Err(err) => tracing::warn!("error parsing relay URL `{relay_url}` from config: {err}"),
        }
    }
    relays
}

pub struct Service {
    host: Ipv4Addr,
    port: u16,
    relays: Vec<RelayEndpoint>,
    network: Network,
}

impl Service {
    pub fn from(config: Config) -> Self {
        let relays = parse_relay_endpoints(&config.relays);

        if relays.is_empty() {
            tracing::error!("no valid relays provided; please restart with correct configuration");
        }

        Self { host: config.host, port: config.port, relays, network: config.network }
    }

    /// Spawns a new [`RelayMux`] and [`BlindedBlockProviderServer`] task
    pub fn spawn(self, context: Option<Context>) -> Result<ServiceHandle, Error> {
        let Self { host, port, relays, network } = self;
        let context =
            if let Some(context) = context { context } else { Context::try_from(&network)? };
        let relays = relays.into_iter().map(Relay::from);
        let clock = context.clock().unwrap_or_else(|| {
            let genesis_time = networks::typical_genesis_time(&context);
            context.clock_at(genesis_time)
        });
        let relay_mux = RelayMux::new(relays, context);

        let relay_mux_clone = relay_mux.clone();
        let relay_task = tokio::spawn(async move {
            let slots = clock.stream_slots();

            tokio::pin!(slots);

            while let Some(slot) = slots.next().await {
                relay_mux_clone.on_slot(slot);
            }
        });

        let server = BlindedBlockProviderServer::new(host, port, relay_mux).spawn();

        Ok(ServiceHandle { relay_mux: relay_task, server })
    }
}

/// Contains the handles to spawned [`RelayMux`] and [`BlindedBlockProviderServer`] tasks
///
/// This struct is created by the [`Service::spawn`] function
#[pin_project::pin_project]
pub struct ServiceHandle {
    #[pin]
    relay_mux: JoinHandle<()>,
    #[pin]
    server: JoinHandle<()>,
}

impl Future for ServiceHandle {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let relay_mux = this.relay_mux.poll(cx);
        if relay_mux.is_ready() {
            return relay_mux
        }
        this.server.poll(cx)
    }
}
