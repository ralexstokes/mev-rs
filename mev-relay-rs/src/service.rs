use crate::relay::Relay;
use beacon_api_client::{mainnet::Client, PayloadAttributesTopic};
use ethereum_consensus::{
    crypto::SecretKey,
    networks::{self, Network},
    primitives::BlsPublicKey,
    state_transition::Context,
};
use futures::StreamExt;
use mev_rs::{blinded_block_provider::Server as BlindedBlockProviderServer, Error};
use serde::Deserialize;
use std::{future::Future, net::Ipv4Addr, pin::Pin, task::Poll};
use tokio::task::{JoinError, JoinHandle};
use tracing::{error, warn};
use url::Url;

#[derive(Deserialize, Debug)]
pub struct Config {
    pub host: Ipv4Addr,
    pub port: u16,
    pub beacon_node_url: String,
    pub secret_key: SecretKey,
    pub accepted_builders: Vec<BlsPublicKey>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: Ipv4Addr::LOCALHOST,
            port: 28545,
            beacon_node_url: "http://127.0.0.1:5052".into(),
            secret_key: Default::default(),
            accepted_builders: Default::default(),
        }
    }
}

pub struct Service {
    host: Ipv4Addr,
    port: u16,
    beacon_node: Client,
    network: Network,
    secret_key: SecretKey,
    accepted_builders: Vec<BlsPublicKey>,
}

impl Service {
    pub fn from(network: Network, config: Config) -> Self {
        let endpoint: Url = config.beacon_node_url.parse().unwrap();
        let beacon_node = Client::new(endpoint);
        Self {
            host: config.host,
            port: config.port,
            beacon_node,
            network,
            secret_key: config.secret_key,
            accepted_builders: config.accepted_builders,
        }
    }

    /// Configures the [`Relay`] and the [`BlindedBlockProviderServer`] and spawns both to
    /// individual tasks
    pub async fn spawn(self) -> Result<ServiceHandle, Error> {
        let Self { host, port, beacon_node, network, secret_key, accepted_builders } = self;

        let context = Context::try_from(network)?;
        let clock = context.clock().unwrap_or_else(|| {
            let genesis_time = networks::typical_genesis_time(&context);
            context.clock_at(genesis_time)
        });
        let relay = Relay::new(beacon_node.clone(), secret_key, accepted_builders, context);

        let block_provider = relay.clone();
        let server = BlindedBlockProviderServer::new(host, port, block_provider).spawn();

        let relay_clone = relay.clone();
        let consensus = tokio::spawn(async move {
            let relay = relay_clone;

            let mut stream = match beacon_node.get_events::<PayloadAttributesTopic>().await {
                Ok(events) => events,
                Err(err) => {
                    error!(%err, "could not open payload attributes stream");
                    return
                }
            };

            while let Some(event) = stream.next().await {
                match event {
                    Ok(event) => {
                        if let Err(err) = relay.on_payload_attributes(event.data) {
                            warn!(%err, "could not process payload attributes");
                        }
                    }
                    Err(err) => {
                        warn!(%err, "error reading payload attributes stream");
                    }
                }
            }
        });

        let relay = tokio::spawn(async move {
            let slots = clock.stream_slots();

            tokio::pin!(slots);

            let mut current_epoch = clock.current_epoch().expect("after genesis");
            relay.on_epoch(current_epoch).await;
            while let Some(slot) = slots.next().await {
                let epoch = clock.epoch_for(slot);
                if epoch > current_epoch {
                    current_epoch = epoch;
                    relay.on_epoch(epoch).await;
                }
                relay.on_slot(slot).await;
            }
        });

        Ok(ServiceHandle { relay, server, consensus })
    }
}

/// Contains the handles to spawned [`Relay`] and [`BlindedBlockProviderServer`] tasks
///
/// This struct is created by the [`Service::spawn`] function
#[pin_project::pin_project]
pub struct ServiceHandle {
    #[pin]
    relay: JoinHandle<()>,
    #[pin]
    server: JoinHandle<()>,
    #[pin]
    consensus: JoinHandle<()>,
}

impl Future for ServiceHandle {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let relay = this.relay.poll(cx);
        if relay.is_ready() {
            return relay
        }
        let consensus = this.consensus.poll(cx);
        if consensus.is_ready() {
            return consensus
        }
        this.server.poll(cx)
    }
}
