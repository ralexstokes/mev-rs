use std::ops::Deref;

use beacon_api_client::Client as BeaconClient;
use ethereum_consensus::{
    crypto::Error as CryptoError, primitives::BlsPublicKey, serde::try_bytes_from_hex_str,
};
use mev_rs::blinded_block_provider::Client;
use url::Url;

#[derive(Clone, Debug)]
pub struct RelayEndpoint {
    url: Url,
    public_key: BlsPublicKey,
}

impl TryFrom<Url> for RelayEndpoint {
    type Error = CryptoError;

    fn try_from(url: Url) -> Result<Self, Self::Error> {
        let public_key = try_bytes_from_hex_str(url.username())?;
        let public_key = BlsPublicKey::try_from(&public_key[..])?;

        Ok(Self { url, public_key })
    }
}

#[derive(Clone)]
pub struct Relay {
    api: Client,
    pub(crate) public_key: BlsPublicKey,
}

impl Deref for Relay {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.api
    }
}

impl From<RelayEndpoint> for Relay {
    fn from(value: RelayEndpoint) -> Self {
        let RelayEndpoint { url, public_key } = value;
        Self { api: Client::new(BeaconClient::new(url)), public_key }
    }
}

#[cfg(test)]
mod tests {
    use ethereum_consensus::crypto::SecretKey;

    use super::*;

    const URL: &str = "https://relay.com";

    #[test]
    fn parse_relay_endpoint() {
        let mut rng = rand::thread_rng();
        let sk = SecretKey::random(&mut rng).unwrap();
        let public_key = sk.public_key();

        let mut url = Url::parse(URL).unwrap();
        url.set_username(&public_key.to_string()).unwrap();

        let endpoint = RelayEndpoint::try_from(url.clone()).unwrap();
        assert_eq!(endpoint.url, url);
        assert_eq!(endpoint.public_key, public_key);
    }

    #[test]
    #[should_panic]
    fn parse_relay_endpoint_missing_public_key() {
        let url = Url::parse(URL).unwrap();
        RelayEndpoint::try_from(url).unwrap();
    }
}
