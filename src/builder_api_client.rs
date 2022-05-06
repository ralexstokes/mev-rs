use crate::builder_api_server::JSON_RPC_RESPONSE_SUCCESS;
use crate::types::{
    BidRequest, BuilderBidV1, ExecutionPayload, SignedBlindedBeaconBlock,
    SignedValidatorRegistration,
};
use ethers_providers::{Http as HttpClient, HttpClientError, JsonRpcClient};
use std::net::SocketAddr;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum Error {
    #[error("error registering validator: {0}")]
    RegistrationError(String),
    #[error("{0}")]
    JsonRpcError(#[from] HttpClientError),
}

// TODO write as `beacon-api-client` with "extension" methods
pub struct Client {
    client: HttpClient,
}

impl Client {
    pub fn new(address: &SocketAddr) -> Self {
        let url = format!("http://{address}");
        let url = Url::parse(&url).unwrap();
        let client = HttpClient::new(url);
        Self { client }
    }

    pub async fn register_validator(
        &self,
        registration: &SignedValidatorRegistration,
    ) -> Result<(), Error> {
        let response = self
            .client
            .request("builder_registerValidatorV1", registration)
            .await?;
        if response == JSON_RPC_RESPONSE_SUCCESS {
            Ok(())
        } else {
            Err(Error::RegistrationError(response))
        }
    }

    pub async fn fetch_bid(&self, bid_request: &BidRequest) -> Result<BuilderBidV1, Error> {
        let bid = self
            .client
            .request("builder_getHeaderV1", bid_request)
            .await?;
        Ok(bid)
    }

    pub async fn accept_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let execution_payload = self
            .client
            .request("builder_getPayloadV1", signed_block)
            .await?;
        Ok(execution_payload)
    }
}
