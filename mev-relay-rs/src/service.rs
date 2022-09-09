use crate::relay::Relay;
use beacon_api_client::ethereum_consensus::state_transition::Context;
use beacon_api_client::Client;
use futures::future::join_all;
use mev_build_rs::{BlindedBlockProviderServer, EngineBuilder, Network};
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
    beacon_node: Client,
    context: Arc<Context>,
}

impl Service {
    pub fn from(config: Config, network: Network) -> Self {
        let endpoint: Url = config.beacon_node_url.parse().unwrap();
        let beacon_node = Client::new(endpoint);
        let context: Context = network.into();
        Self {
            host: config.host,
            port: config.port,
            beacon_node,
            context: Arc::new(context),
        }
    }

    pub async fn run(&self) {
        let builder = EngineBuilder::new(self.context.clone());
        let relay = Relay::new(builder, self.beacon_node.clone(), self.context.clone());
        relay.initialize().await;

        let block_provider = relay.clone();
        let api_server = BlindedBlockProviderServer::new(self.host, self.port, block_provider);

        let mut tasks = vec![];
        tasks.push(tokio::spawn(async move {
            api_server.run().await;
        }));
        tasks.push(tokio::spawn(async move {
            relay.run().await;
        }));
        join_all(tasks).await;
    }
}
