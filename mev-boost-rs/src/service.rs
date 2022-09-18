use crate::relay_mux::RelayMux;
use beacon_api_client::Client;
use ethereum_consensus::state_transition::Context;
use mev_build_rs::{BlindedBlockProviderClient as Relay, BlindedBlockProviderServer, Network};
use serde::Deserialize;
use std::{future::Future, net::Ipv4Addr, pin::Pin, sync::Arc, task::Poll};
use tokio::task::{JoinError, JoinHandle};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub relays: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self { host: Ipv4Addr::UNSPECIFIED, port: 18550, relays: vec![] }
    }
}

fn parse_url(input: &str) -> Option<Url> {
    if input.is_empty() {
        None
    } else {
        input
            .parse()
            .map_err(|err| {
                tracing::warn!("error parsing relay from URL: `{err}`");
                err
            })
            .ok()
    }
}

pub struct Service {
    host: Ipv4Addr,
    port: u16,
    relays: Vec<Url>,
    network: Network,
}

impl Service {
    pub fn from(config: Config, network: Network) -> Self {
        let relays: Vec<Url> = config.relays.iter().filter_map(|s| parse_url(s)).collect();

        if relays.is_empty() {
            tracing::error!("no valid relays provided; please restart with correct configuration");
        }

        Self { host: config.host, port: config.port, relays, network }
    }

    /// Spawns a new [`RelayMux`] and [`BlindedBlockProviderServer`] task
    pub fn spawn(self) -> ServiceHandle {
        let Self { host, port, relays, network } = self;
        let context: Context = self.network.into();
        let relays = relays.into_iter().map(|endpoint| Relay::new(Client::new(endpoint)));
        let relay_mux = RelayMux::new(relays, Arc::new(context), network);

        let relay_mux_clone = relay_mux.clone();
        let relayer = tokio::spawn(async move {
            relay_mux_clone.run().await;
        });

        let server = tokio::spawn(async move {
            let server = BlindedBlockProviderServer::new(host, port, relay_mux);
            server.run().await;
        });

        ServiceHandle { relayer, server }
    }
}

/// Contains the handles to spawned [`RelayMux`] and [`BlindedBlockProviderServer`] tasks
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
