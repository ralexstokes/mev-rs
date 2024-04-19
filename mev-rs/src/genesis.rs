use beacon_api_client::mainnet::Client;
use ethereum_consensus::{networks::typical_genesis_time, state_transition::Context};
use tracing::warn;
use url::Url;

pub async fn get_genesis_time(
    context: &Context,
    beacon_node_url: Option<&String>,
    beacon_node: Option<&Client>,
) -> u64 {
    match context.genesis_time() {
        Ok(genesis_time) => genesis_time,
        Err(_) => {
            // use provided beacon node
            if let Some(client) = beacon_node {
                if let Ok(genesis_details) = client.get_genesis_details().await {
                    return genesis_details.genesis_time
                }
            }

            // use provided url for beacon node
            if let Some(url) = beacon_node_url {
                if let Ok(url) = Url::parse(url) {
                    let client = Client::new(url);
                    if let Ok(genesis_details) = client.get_genesis_details().await {
                        return genesis_details.genesis_time
                    }
                }
            }

            // fallback
            warn!("could not get genesis time from context or connection to consensus node; using best guess");
            typical_genesis_time(context)
        }
    }
}
