use beacon_api_client::mainnet::Client as BeaconApiClient;
use ethereum_consensus::{primitives::BlsPublicKey, serde::try_bytes_from_hex_str};
use url::Url;

use crate::{
    blinded_block_provider::Client as BlindedBlockProvider,
    blinded_block_relayer::Client as BlindedBlockRelayer, relay::Relay,
};

/// Mock relay endpoint for testing.
pub const RELAY_URL: &str = "https://0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a@boost-relay-sepolia.flashbots.net/";

/// Creates a [`BlsPublicKey`] for testing.
pub fn test_public_key() -> BlsPublicKey {
    let bytes = try_bytes_from_hex_str("0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a").unwrap();
    BlsPublicKey::try_from(bytes.as_ref()).unwrap()
}

/// Creates a mock relay endpoint [`Url`] for testing.
pub fn test_endpoint() -> Url {
    Url::parse(RELAY_URL).unwrap()
}

/// Mock relay for testing
pub fn test_relay() -> Relay {
    Relay {
        provider: test_blinded_block_provider(),
        relayer: test_blinded_block_relayer(),
        public_key: test_public_key(),
        endpoint: test_endpoint(),
    }
}

/// Creates a mock [`BeaconApiClient`] for testing.
pub fn test_beacon_api_client() -> BeaconApiClient {
    // TODO: we need to intercept requests to stub mock responses
    BeaconApiClient::new(test_endpoint())
}

/// Creates a [`BlindedBlockProvider`] for testing.
pub fn test_blinded_block_provider() -> BlindedBlockProvider {
    BlindedBlockProvider::new(test_beacon_api_client())
}

/// Creates a mock [`BlindedBlockRelayer`] for testing.
pub fn test_blinded_block_relayer() -> BlindedBlockRelayer {
    BlindedBlockRelayer::new(test_beacon_api_client())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_public_key() {
        let public_key = test_public_key();
        let bytes = try_bytes_from_hex_str("0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a").unwrap();
        assert_eq!(public_key, BlsPublicKey::try_from(bytes.as_ref()).unwrap());
    }

    #[test]
    fn test_test_endpoint() {
        let endpoint = test_endpoint();
        assert_eq!(endpoint.as_str(), RELAY_URL);
    }

    #[test]
    fn test_test_relay() {
        let relay = test_relay();
        assert_eq!(relay.endpoint.as_str(), RELAY_URL);
        assert_eq!(relay.public_key, test_public_key());
    }
}
