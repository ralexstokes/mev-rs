use futures::future::join_all;
use reqwest::Client;
use std::net::{Ipv4Addr, SocketAddr};

mod builder_api;
mod relay;
mod relay_mux;
mod types;

use builder_api::Server as BuilderApiServer;
use relay::Relay;
use relay_mux::RelayMux;

pub struct ServiceConfig {
    pub host: Ipv4Addr,
    pub port: u16,
    pub relays: Vec<SocketAddr>,
}

pub struct Service {
    config: ServiceConfig,
}

impl Service {
    pub fn from(config: ServiceConfig) -> Self {
        Self { config }
    }

    pub async fn run(&mut self) {
        let http_client = Client::new();

        let relays = self
            .config
            .relays
            .iter()
            .map(|addr| Relay::new(http_client.clone(), addr))
            .collect::<Vec<_>>();
        let relay_channels = relays
            .iter()
            .map(|relay| relay.channel())
            .collect::<Vec<_>>();

        let relay_mux = RelayMux::new(relay_channels);

        let mut tasks = vec![];
        for mut relay in relays.into_iter() {
            tasks.push(tokio::spawn(async move {
                relay.run().await;
            }));
        }

        let relay_mux_clone = relay_mux.clone();
        tasks.push(tokio::spawn(async move {
            relay_mux.run().await;
        }));

        let mut builder_api = BuilderApiServer::new(self.config.host, self.config.port);
        tasks.push(tokio::spawn(async move {
            builder_api.run(relay_mux_clone).await;
        }));

        join_all(tasks).await;
    }
}
