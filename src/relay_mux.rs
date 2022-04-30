use crate::relay::{Relay, RelayError};
use crate::types::{
    BuilderBidV1, ExecutionPayload, ProposalRequest, SignedBlindedBeaconBlock,
    ValidatorRegistrationV1,
};
use futures::future::join_all;

pub struct RelayMux {
    relays: Vec<Relay>,
}

impl RelayMux {
    pub fn over(relays: impl Iterator<Item = Relay>) -> Self {
        Self {
            relays: relays.collect(),
        }
    }

    pub async fn connect_to_all(&mut self) {
        if self.relays.is_empty() {
            tracing::error!(
                "no relays provided, please restart with at least one relay configured"
            );
            return;
        }

        let connection_results =
            join_all(self.relays.iter_mut().map(|relay| relay.connect())).await;
        for result in connection_results {
            if let Err(err) = result {
                tracing::warn!("{err}]");
            }
        }
    }

    pub async fn register_validator(
        &self,
        registration: &ValidatorRegistrationV1,
    ) -> Result<(), Vec<RelayError>> {
        // TODO: validations
        // TODO: call to relays
        // TODO: return any errors
        Ok(())
    }

    pub async fn fetch_best_header(
        &self,
        proposal_request: &ProposalRequest,
    ) -> Result<BuilderBidV1, Vec<RelayError>> {
        // TODO: fetch headers from all relays
        // TODO: return the most profitable one
        Ok(BuilderBidV1 { a: 12 })
    }

    pub async fn post_block(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Vec<RelayError>> {
        // TODO: post the block
        // TODO: return the execution payload
        Ok(ExecutionPayload { a: 12 })
    }
}
