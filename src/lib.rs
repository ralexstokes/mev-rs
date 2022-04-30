use reqwest::Client;
use std::net::{Ipv4Addr, SocketAddr};

mod json_rpc_server;
mod relay;
mod relay_mux;
mod types;

use json_rpc_server::Server as JsonRpcServer;
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
            .map(|addr| Relay::new(http_client.clone(), addr));
        let mut relay_mux = RelayMux::over(relays);
        relay_mux.connect_to_all().await;

        let mut json_rpc_server = JsonRpcServer::new(self.config.host, self.config.port, relay_mux);
        json_rpc_server.run().await;
    }
}
