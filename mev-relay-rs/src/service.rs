use crate::relay::Relay;
use ethereum_consensus::state_transition::Context;
use mev_build_rs::BlindedBlockProviderServer;
use serde::Deserialize;
use std::net::Ipv4Addr;

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
        let relay = Relay::new(context);
        let api_server = BlindedBlockProviderServer::new(self.host, self.port, relay);
        api_server.run().await;
    }
}
