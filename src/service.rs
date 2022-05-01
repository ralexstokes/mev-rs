use crate::builder_api_client::Client as Relay;
use crate::builder_api_server::Server as ApiServer;
use crate::relay_mux::RelayMux;
use futures::future::join_all;
use std::net::{Ipv4Addr, SocketAddr};

#[derive(Debug)]
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
        let relays = self
            .config
            .relays
            .iter()
            .map(|addr| Relay::new(addr))
            .collect::<Vec<_>>();
        let relay_mux = RelayMux::new(relays);

        let mut tasks = vec![];

        let relay_mux_clone = relay_mux.clone();
        tasks.push(tokio::spawn(async move {
            relay_mux.run().await;
        }));

        let mut builder_api = ApiServer::new(self.config.host, self.config.port);
        tasks.push(tokio::spawn(async move {
            builder_api.run(relay_mux_clone).await;
        }));

        join_all(tasks).await;
    }
}
