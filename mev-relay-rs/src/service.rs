use crate::relay::Relay;
use beacon_api_client::Client;
use ethereum_consensus::state_transition::Context;
use mev_build_rs::{BlindedBlockProviderServer, EngineBuilder};
use serde::Deserialize;
use std::net::Ipv4Addr;
use std::sync::Arc;
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
    _beacon_node: Client,
    builder: EngineBuilder,
    context: Arc<Context>,
}

impl Service {
    pub fn from(config: Config) -> Self {
        let endpoint: Url = config.beacon_node_url.parse().unwrap();
        let beacon_node = Client::new(endpoint);
        let context = Arc::new(Context::for_mainnet());
        let builder = EngineBuilder::new(context.clone());
        Self {
            host: config.host,
            port: config.port,
            _beacon_node: beacon_node,
            builder,
            context,
        }
    }

    pub async fn run(&self) {
        let relay = Relay::new(self.builder.clone(), self.context.clone());
        let api_server = BlindedBlockProviderServer::new(self.host, self.port, relay);
        api_server.run().await;
    }
}
