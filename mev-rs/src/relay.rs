use crate::{
    blinded_block_provider::Client as BlockProvider,
    blinded_block_relayer::{BlindedBlockRelayer, Client as Relayer},
    error::Error,
    types::{ProposerSchedule, SignedBidSubmission},
};
use async_trait::async_trait;
use beacon_api_client::Client as BeaconClient;
use ethereum_consensus::{
    crypto::Error as CryptoError, primitives::BlsPublicKey, serde::try_bytes_from_hex_str,
};
use std::{cmp, fmt, hash, ops::Deref};
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

/// A wrapper around a vector of [`RelayEndpoint`]s.
#[derive(Clone, Debug)]
pub struct RelayEndpoints(Vec<RelayEndpoint>);

impl RelayEndpoints {
    pub fn iter(&self) -> impl Iterator<Item = &RelayEndpoint> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl IntoIterator for RelayEndpoints {
    type Item = RelayEndpoint;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<T> From<Vec<T>> for RelayEndpoints
where
    T: AsRef<str>,
{
    fn from(value: Vec<T>) -> Self {
        Self::from(value.as_slice())
    }
}

impl<T> From<&Vec<T>> for RelayEndpoints
where
    T: AsRef<str>,
{
    fn from(value: &Vec<T>) -> Self {
        Self::from(value.as_slice())
    }
}

impl<T> From<&[T]> for RelayEndpoints
where
    T: AsRef<str>,
{
    fn from(value: &[T]) -> Self {
        let mut relays = vec![];
        for endpoint in value {
            let e = endpoint.as_ref();
            let url = match Url::parse(e) {
                Ok(url) => url,
                Err(err) => {
                    tracing::error!(%err, %e, "error parsing relay URL from config");
                    continue;
                }
            };
            match RelayEndpoint::try_from(url) {
                Ok(relay) => relays.push(relay),
                Err(err) => {
                    tracing::warn!(%err, %e, "error parsing relay from URL")
                }
            }
        }
        if relays.is_empty() {
            tracing::error!(
                "no relays could be loaded from the configuration; please fix and restart"
            );
        }
        RelayEndpoints(relays)
    }
}

impl fmt::Display for RelayEndpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        f.write_str(self.url.as_str())
    }
}

#[derive(Clone)]
pub struct Relay {
    provider: BlockProvider,
    relayer: Relayer,
    pub public_key: BlsPublicKey,
    pub endpoint: Url,
}

impl hash::Hash for Relay {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.public_key.hash(state);
    }
}

impl cmp::PartialEq for Relay {
    fn eq(&self, other: &Self) -> bool {
        self.public_key == other.public_key
    }
}

impl cmp::Eq for Relay {}

impl fmt::Debug for Relay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.endpoint.as_str())
    }
}

impl fmt::Display for Relay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Deref for Relay {
    type Target = BlockProvider;

    fn deref(&self) -> &Self::Target {
        &self.provider
    }
}

impl From<RelayEndpoint> for Relay {
    fn from(value: RelayEndpoint) -> Self {
        let RelayEndpoint { url, public_key } = value;
        let endpoint = url.clone();
        let api_client = BeaconClient::new(url);
        let provider = BlockProvider::new(api_client.clone());
        let relayer = Relayer::new(api_client.clone());
        Self { provider, relayer, public_key, endpoint }
    }
}

#[async_trait]
impl BlindedBlockRelayer for Relay {
    async fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error> {
        self.relayer.get_proposal_schedule().await
    }

    async fn submit_bid(&self, signed_submission: &mut SignedBidSubmission) -> Result<(), Error> {
        self.relayer.submit_bid(signed_submission).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethereum_consensus::crypto::SecretKey;

    const URL: &str = "https://relay.com";
    const RELAY_URL: &str = "https://0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a@boost-relay-sepolia.flashbots.net";

    #[test]
    fn test_relay_endpoints_from_vec() {
        let endpoints = vec![URL, RELAY_URL];
        let relay_endpoints = RelayEndpoints::from(endpoints);
        assert_eq!(relay_endpoints.len(), 1);
    }

    #[test]
    fn test_empty_relay_endpoints_from_vec() {
        let endpoints = vec![URL];
        let relay_endpoints = RelayEndpoints::from(endpoints);
        assert_eq!(relay_endpoints.len(), 0);
    }

    #[test]
    fn parse_relay_endpoint() {
        let mut rng = rand::thread_rng();
        let sk = SecretKey::random(&mut rng).unwrap();
        let public_key = sk.public_key();

        let mut url = Url::parse(URL).unwrap();
        let public_key_str = format!("{public_key:?}");
        url.set_username(&public_key_str).unwrap();

        let endpoint = RelayEndpoint::try_from(url.clone()).unwrap();
        assert_eq!(endpoint.url, url);
        assert_eq!(endpoint.public_key, public_key);
    }

    #[test]
    fn parse_live_relay() {
        let url = Url::parse(RELAY_URL).unwrap();
        let endpoint = RelayEndpoint::try_from(url.clone()).unwrap();
        assert_eq!(endpoint.url, url);
        let bytes = try_bytes_from_hex_str("0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a").unwrap();
        assert_eq!(endpoint.public_key, BlsPublicKey::try_from(bytes.as_ref()).unwrap());
    }

    #[test]
    #[should_panic]
    fn parse_relay_endpoint_missing_public_key() {
        let url = Url::parse(URL).unwrap();
        RelayEndpoint::try_from(url).unwrap();
    }
}
