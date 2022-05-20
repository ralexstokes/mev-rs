use crate::relay::Relay;
use ethereum_consensus::phase0::mainnet::Context;
use mev_build_rs::ApiServer;
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::UNSPECIFIED,
            port: 28545,
        }
    }
}

pub struct Service {
    host: Ipv4Addr,
    port: u16,
}

impl Service {
    pub fn from(config: Config) -> Self {
        Self {
            host: config.host,
            port: config.port,
        }
    }

    pub async fn run(&self) {
        let context = Context::default();
        let relay = Relay::new(Arc::new(context));
        let api_server = ApiServer::new(self.host, self.port, relay);
        api_server.run().await;
    }
}
