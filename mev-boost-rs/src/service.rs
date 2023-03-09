use crate::{relay_mux::RelayMux, relay_struct::RelayStruct};
use beacon_api_client::Client;
use ethereum_consensus::{primitives::BlsPublicKey, state_transition::Context};
use futures::StreamExt;
use mev_rs::{
    blinded_block_provider::{Client as Relay, Server as BlindedBlockProviderServer},
    Error, Network,
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
    relays: Vec<RelayStruct>,
    network: Network,
}

impl Service {
    pub fn from(config: Config) -> Self {
        let relays_url: Vec<Url> = config.relays.iter().filter_map(|s| parse_url(s)).collect();

        if relays_url.is_empty() {
            tracing::error!(
                "no valid relays URL provided; please restart with correct configuration"
            );
        }

        let relays: Vec<RelayStruct> = relays_url
            .iter()
            .filter_map(|endpoint| {
                Some(RelayStruct::new(
                    Relay::new(Client::new(endpoint.clone()), BlsPublicKey::default()),
                    BlsPublicKey::default(),
                    url::Url::from(endpoint.clone()),
                ))
            })
            .collect();

        Self { host: config.host, port: config.port, relays, network: config.network }
    }

    /// Spawns a new [`RelayMux`] and [`BlindedBlockProviderServer`] task
    pub fn spawn(self, context: Option<Context>) -> Result<ServiceHandle, Error> {
        let Self { host, port, relays, network } = self;
        let context =
            if let Some(context) = context { context } else { Context::try_from(&network)? };
        let relays = relays
            .into_iter()
            .map(|relay| Relay::new(Client::new(relay.endpoint), BlsPublicKey::default()));
        let clock = context.clock(None);
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
            return relay_mux;
        }
        this.server.poll(cx)
    }
}
