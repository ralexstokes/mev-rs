use beacon_api_client::Client as BeaconApiClient;
use ethereum_consensus::primitives::BlsPublicKey;
use mev_rs::{blinded_block_provider::Client as RelayClient, Error as RelayError};
use std::str::FromStr;
use url::Url;

//TODO: rename to relay and change type alias in relayMux etc.
#[derive(Clone)]
pub struct RelayEntry {
    pub api: RelayClient,
    pub public_key: BlsPublicKey,
}

impl RelayEntry {
    pub fn new(endpoint: Url, public_key: BlsPublicKey) -> Self {
        let client_api = BeaconApiClient::new(endpoint);
        Self { api: RelayClient::new(client_api, public_key.clone()), public_key }
    }
}

impl TryFrom<Url> for RelayEntry {
    type Error = RelayError;

    fn try_from(url: Url) -> Result<Self, Self::Error> {
        if url.username().is_empty() || url.username().len() != 98 {
            return Err(RelayError::RelayUrlPublicKeyError(url, "public key field of relay URL is incorrectly formed: public key must be 48 characters in length".to_string()));
        }

        match hex::decode(url.username().replace("0x", "")) {
            Ok(hex) => match BlsPublicKey::try_from(hex.as_ref()) {
                Ok(public_key) => Ok(Self::new(url, public_key)),
                Err(e) => Err(RelayError::RelayUrlPublicKeyError(
                    url,
                    format!("unable to parse hex data to public key {e}"),
                )),
            },
            Err(e) => Err(RelayError::RelayUrlPublicKeyError(
                url,
                format!("unable to decode public key hex data {e}"),
            )),
        }
    }
}

impl TryFrom<&String> for RelayEntry {
    type Error = RelayError;

    fn try_from(str: &String) -> Result<Self, Self::Error> {
        match Url::parse(str) {
            Ok(url) => RelayEntry::try_from(url),
            Err(e) => Err(RelayError::RelayUrlParseError(str.to_owned(), e)),
        }
    }
}

impl FromStr for RelayEntry {
    type Err = RelayError;

    fn from_str(str: &str) -> Result<Self, Self::Err> {
        match Url::parse(str) {
            Ok(url) => RelayEntry::try_from(url),
            Err(e) => Err(RelayError::RelayUrlParseError(String::from(str), e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_relay_from_str_url() {
        let test_url = format!("http://{0}@127.0.0.1:5555", BlsPublicKey::default().to_string());
        let result = RelayEntry::from_str(&test_url).unwrap();
        let expected = RelayEntry {
            api: RelayClient::new(
                BeaconApiClient::new(Url::parse(&test_url).unwrap()),
                BlsPublicKey::default(),
            ),
            public_key: BlsPublicKey::default(),
        };
        //TODO: BeaconApiClient does not implement Eq -> change this to allow us to derive Eq
        assert_eq!(result.public_key, expected.public_key);
    }

    #[test]
    fn test_parse_relay_try_from_string_url() {
        let test_url = format!("http://{0}@127.0.0.1:5555", BlsPublicKey::default().to_string(),);
        let result = RelayEntry::try_from(&test_url).unwrap();
        let expected = RelayEntry {
            api: RelayClient::new(
                BeaconApiClient::new(Url::parse(&test_url).unwrap()),
                BlsPublicKey::default(),
            ),
            public_key: BlsPublicKey::default(),
        };
        //TODO: BeaconApiClient does not implement Eq -> change this to allow us to derive Eq
        assert_eq!(result.public_key, expected.public_key);
    }

    #[test]
    fn test_url_parse_errors() {
        let public_key = "0x000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001";
        let long_public_key = "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000011";
        let short_public_key = "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001";
        let http = "http://";
        let host_name = "@127.0.0.1:5555";
        let bad_host_name = "@127.0.0.1:555a";

        let test_cases = [
            format!(""),
            format!("{http}{host_name}"),
            format!("{http}{public_key}"),
            format!("{public_key}{host_name}"),
            format!("{http}{long_public_key}{host_name}"),
            format!("{http}{short_public_key}{host_name}"),
            format!("{http}{public_key}{bad_host_name}"),
        ];

        for input in test_cases.into_iter() {
            let output = RelayEntry::try_from(&input);
            //TODO: Implement PartialEq/Eq for BeaconApiClient to test returned Errors of
            // RelayEntry via assert_eq!()
            assert!(output.is_err());
        }
    }
}
