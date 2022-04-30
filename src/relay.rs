use crate::types::{BuilderBidV1, ProposalRequest, ValidatorRegistrationV1};
use reqwest::{Client, Error, StatusCode};
use std::net::SocketAddr;
use thiserror::Error;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;

#[derive(Debug, Error)]
pub enum RelayError {
    #[error("{0}")]
    HTTPError(#[from] Error),
    #[error("simple request to relay failed")]
    CouldNotPing,
    #[error("tokio channel dropped: {0}")]
    Tokio(#[from] oneshot::error::RecvError),
}

#[derive(Debug)]
pub enum Message {
    Registration(
        ValidatorRegistrationV1,
        oneshot::Sender<Result<(), RelayError>>,
    ),
    FetchBid(
        ProposalRequest,
        oneshot::Sender<Result<BuilderBidV1, RelayError>>,
    ),
}

pub struct Relay {
    client: Client,
    endpoint: String,

    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

impl Relay {
    pub fn new(client: Client, address: &SocketAddr) -> Self {
        let endpoint = format!("https://{address}");
        let (sender, receiver) = mpsc::channel(16);
        Self {
            client,
            endpoint,
            sender,
            receiver,
        }
    }

    pub fn channel(&self) -> mpsc::Sender<Message> {
        self.sender.clone()
    }

    pub async fn run(&mut self) {
        while let Some(msg) = self.receiver.recv().await {
            match msg {
                Message::Registration(registration, resp) => {
                    let response = self.register(&registration).await;
                    if let Err(err) = resp.send(response) {
                        tracing::warn!("relay caller dropped: {err:?}");
                    }
                }
                Message::FetchBid(proposal, resp) => {
                    let response = self.fetch_bid(&proposal).await;
                    if let Err(err) = resp.send(response) {
                        tracing::warn!("relay caller dropped: {err:?}");
                    }
                }
            }
        }
    }

    pub async fn register(&self, registration: &ValidatorRegistrationV1) -> Result<(), RelayError> {
        Ok(())
        // let host = &self.endpoint;
        // let endpoint = format!("{host}/register");
        // let response = self.client.get(endpoint).send().await?;
        // if response.status() == StatusCode::OK {
        //     Ok(RelayResult::Registration)
        // } else {
        //     Err(RelayError::CouldNotPing)
        // }
    }

    pub async fn fetch_bid(
        &self,
        proposal_request: &ProposalRequest,
    ) -> Result<BuilderBidV1, RelayError> {
        Ok(BuilderBidV1 { value: 12 })
    }
}
