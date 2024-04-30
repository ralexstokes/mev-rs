//! Resolve a given payload for use in the auction
//! Takes a payload from the payload builder and "finalizes" the crafted payload to yield a valid
//! block according to the auction rules.

use crate::payload::builder::PayloadBuilder;
use futures_util::FutureExt;
use reth::{
    payload::{error::PayloadBuilderError, EthBuiltPayload, PayloadId},
    primitives::{Address, SealedBlock, B256, U256},
    providers::StateProviderFactory,
    revm::primitives::{BlockEnv, CfgEnvWithHandlerCfg},
};
use reth_basic_payload_builder::ResolveBestPayload;
use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};

#[derive(Debug)]
pub struct PayloadFinalizerConfig {
    pub proposer_fee_recipient: Address,
    pub parent_hash: B256,
    // TODO: store with payload builder?
    pub cfg_env: CfgEnvWithHandlerCfg,
    // TODO: store with payload builder?
    pub block_env: BlockEnv,
}

#[derive(Debug)]
pub struct PayloadFinalizer<Client, Pool> {
    pub client: Client,
    pub _pool: Pool,
    pub payload_id: PayloadId,
    pub builder: PayloadBuilder,
    pub config: Option<PayloadFinalizerConfig>,
}

impl<Client: StateProviderFactory + Clone, Pool> PayloadFinalizer<Client, Pool> {
    fn prepare(
        &self,
        block: SealedBlock,
        fees: U256,
        config: &PayloadFinalizerConfig,
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        // TODO: - track proposer payment, revenue
        self.builder.finalize_payload(self.payload_id, self.client.clone(), block, fees, config)
    }

    fn process(
        &mut self,
        block: SealedBlock,
        fees: U256,
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        let config = self.config.as_ref().expect("always exists");
        self.prepare(block, fees, config)
    }
}

#[derive(Debug)]
pub struct ResolveBuilderPayload<Client, Pool> {
    pub resolution: ResolveBestPayload<EthBuiltPayload>,
    pub finalizer: PayloadFinalizer<Client, Pool>,
}

impl<Client, Pool> Future for ResolveBuilderPayload<Client, Pool>
where
    Client: StateProviderFactory + Clone + Unpin,
    Pool: Unpin,
{
    type Output = Result<EthBuiltPayload, PayloadBuilderError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let payload = ready!(this.resolution.poll_unpin(cx))?;

        if this.finalizer.config.is_none() {
            // This should never be the case, but if it is, then just return the (ineligible)
            // payload
            return Poll::Ready(Ok(payload))
        }

        // TODO: consider making the payment addition `spawn_blocking`

        let block = payload.block().clone();
        let fees = payload.fees();
        let blob_sidecars = payload.sidecars().to_vec();

        // TODO: get amount to bid from bidder
        // TODO: add channel send here to dispatch fees, wait for bidder response

        let finalized_payload = this.finalizer.process(block, fees).map(|mut payload| {
            payload.extend_sidecars(blob_sidecars);
            payload
        });
        Poll::Ready(finalized_payload)
    }
}
