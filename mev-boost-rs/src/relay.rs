use beacon_api_client::Client as BeaconClient;
use ethereum_consensus::{crypto::Error as CryptoError, primitives::BlsPublicKey};
use thiserror::Error;
use url::Url;

use mev_rs::blinded_block_provider::Client;

#[derive(Clone, Debug)]
pub struct RelayEndpoint {
    url: Url,
    public_key: BlsPublicKey,
}

impl RelayEndpoint {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn public_key(&self) -> &BlsPublicKey {
        &self.public_key
    }
}

#[derive(Debug, Error)]
pub enum RelayUrlError {
    #[error("{0}")]
    Bls(#[from] CryptoError),
    #[error("{0}")]
    Hex(#[from] hex::FromHexError),
    #[error("public key {0} missing '0x' hex prefix")]
    Missing0xPrefix(String),
    #[error("URL {0} missing public key username")]
    MissingPublicKey(String),
}

impl TryFrom<Url> for RelayEndpoint {
    type Error = RelayUrlError;

    fn try_from(url: Url) -> Result<Self, Self::Error> {
        let public_key = url.username();
        if public_key.is_empty() {
            return Err(Self::Error::MissingPublicKey(url.to_string()));
        }

        let public_key =
            public_key.strip_prefix("0x").ok_or(Self::Error::Missing0xPrefix(public_key.into()))?;
        let public_key = hex::decode(public_key)?;
        let public_key = BlsPublicKey::try_from(public_key.as_slice())?;

        Ok(Self { url, public_key })
    }
}

#[derive(Clone)]
pub struct Relay {
    api: Client,
    public_key: BlsPublicKey,
}

impl Relay {
    pub fn api(&self) -> &Client {
        &self.api
    }

    pub fn public_key(&self) -> &BlsPublicKey {
        &self.public_key
    }
}

impl From<RelayEndpoint> for Relay {
    fn from(value: RelayEndpoint) -> Self {
        Self {
            api: Client::new(BeaconClient::new(value.url().clone())),
            public_key: value.public_key().clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ethereum_consensus::crypto::SecretKey;

    use std::ops::Deref;

    const URL: &'static str = "https://relay.com";

    fn random_bls_public_key() -> BlsPublicKey {
        let mut rng = rand::thread_rng();
        let sk = SecretKey::random(&mut rng).unwrap();
        sk.public_key()
    }

    #[test]
    fn parse_relay_endpoint() {
        let public_key = random_bls_public_key();
        let public_key_hex = format!("{:#x}", public_key.deref());

        let mut url = Url::parse(URL).unwrap();
        url.set_username(&public_key_hex).unwrap();

        let endpoint = RelayEndpoint::try_from(url.clone()).unwrap();
        assert_eq!(endpoint.url, url);
        assert_eq!(endpoint.public_key, public_key);
    }

    #[test]
    fn parse_relay_endpoint_missing_public_key() {
        let url = Url::parse(URL).unwrap();

        let endpoint = RelayEndpoint::try_from(url.clone());
        assert!(std::matches!(endpoint, Err(RelayUrlError::MissingPublicKey(..))));
    }

    #[test]
    fn parse_relay_endpoint_missing_0x_prefix() {
        let public_key = random_bls_public_key();

        // Format public key without '0x' prefix.
        let public_key_hex = format!("{:x}", public_key.deref());

        let mut url = Url::parse(URL).unwrap();
        url.set_username(&public_key_hex).unwrap();

        let endpoint = RelayEndpoint::try_from(url.clone());
        assert!(std::matches!(endpoint, Err(RelayUrlError::Missing0xPrefix(..))));
    }

    #[test]
    fn parse_relay_endpoint_invalid_hex() {
        // Use string with proper '0x' prefix but invalid hex.
        let invalid_hex = "0xethereum";

        let mut url = Url::parse(URL).unwrap();
        url.set_username(&invalid_hex).unwrap();

        let endpoint = RelayEndpoint::try_from(url.clone());
        assert!(std::matches!(endpoint, Err(RelayUrlError::Hex(..))));
    }

    #[test]
    fn parse_relay_endpoint_invalid_bls() {
        let public_key = random_bls_public_key();

        // Append some extra hex to the BLS public key.
        let extra = "00";
        let invalid_public_key_hex = format!("{:#x}{extra}", public_key.deref());

        let mut url = Url::parse(URL).unwrap();
        url.set_username(&invalid_public_key_hex).unwrap();

        let endpoint = RelayEndpoint::try_from(url.clone());
        assert!(std::matches!(endpoint, Err(RelayUrlError::Bls(..))));
    }
}
