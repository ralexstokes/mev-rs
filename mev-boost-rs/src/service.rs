use crate::relay_mux::RelayMux;
use beacon_api_client::Client;
use ethereum_consensus::state_transition::Context;
use futures::future::join_all;
use mev_build_rs::{BlindedBlockProviderClient as Relay, BlindedBlockProviderServer, Network};
use serde::Deserialize;
use std::{net::Ipv4Addr, sync::Arc};
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

    pub async fn run(&self) {
        let context: Context = self.network.into();
        let relays = self.relays.iter().cloned().map(|endpoint| Relay::new(Client::new(endpoint)));
        let relay_mux = RelayMux::new(relays, Arc::new(context), self.network);

        let mut tasks = vec![];

        let relay_mux_clone = relay_mux.clone();
        tasks.push(tokio::spawn(async move {
            relay_mux_clone.run().await;
        }));

        let builder_api = BlindedBlockProviderServer::new(self.host, self.port, relay_mux);
        tasks.push(tokio::spawn(async move {
            builder_api.run().await;
        }));

        join_all(tasks).await;
    }
}
