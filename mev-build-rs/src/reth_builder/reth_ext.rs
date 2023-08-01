/// Implement the required functionality to interface with the `reth` payload builder
/// functionality, primarily `PayloadJobGenerator`.
///
/// This module essentially implements a "no-op" builder from the point of view of `reth`,
/// and provides a touch point to signal new payload attributes to this crate's builder.
use crate::reth_builder::builder::Builder;
use futures::FutureExt;
use reth_payload_builder::{
    error::PayloadBuilderError, BuiltPayload, KeepPayloadJobAlive, PayloadBuilderAttributes,
    PayloadId, PayloadJob, PayloadJobGenerator,
};
use reth_primitives::{SealedBlock, U256};
use reth_provider::{BlockReaderIdExt, StateProviderFactory};
use reth_transaction_pool::TransactionPool;
use std::{
    future::{self, Future, Ready},
    pin::Pin,
    sync::Arc,
    task::Poll,
};

// `Send` and `Sync` so we can have builder implement the required `reth` payload builder traits.
unsafe impl<Pool, Client> Send for Builder<Pool, Client> {}
unsafe impl<Pool, Client> Sync for Builder<Pool, Client> {}

type Sender = dyn Future<Output = ()> + Send + Sync;

pub struct Job {
    payload_id: PayloadId,
    send_fut: Pin<Box<Sender>>,
}

impl Future for Job {
    type Output = Result<(), PayloadBuilderError>;

    fn poll(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        match this.send_fut.poll_unpin(cx) {
            Poll::Ready(_) => Poll::Ready(Ok(())),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl<Pool: TransactionPool, Provider: StateProviderFactory + BlockReaderIdExt> PayloadJobGenerator
    for Builder<Pool, Provider>
{
    type Job = Job;

    fn new_payload_job(
        &self,
        attr: PayloadBuilderAttributes,
    ) -> Result<Self::Job, PayloadBuilderError> {
        let payload_id = attr.payload_id();
        let tx = self.payload_attributes_tx.clone();
        let send_fut = Box::pin(async move {
            if let Err(err) = tx.send(attr).await {
                let attr = err.0;
                tracing::warn!(timestamp = ?attr.timestamp, id = %attr.payload_id(), "could not send attributes");
            }
        });
        Ok(Job { payload_id, send_fut })
    }
}

impl PayloadJob for Job {
    type ResolvePayloadFuture = Ready<Result<Arc<BuiltPayload>, PayloadBuilderError>>;

    fn best_payload(&self) -> Result<Arc<BuiltPayload>, PayloadBuilderError> {
        let payload = Arc::new(build_default_payload(self.payload_id));
        Ok(payload)
    }

    fn resolve(&mut self) -> (Self::ResolvePayloadFuture, KeepPayloadJobAlive) {
        let payload = Arc::new(build_default_payload(self.payload_id));
        (future::ready(Ok(payload)), KeepPayloadJobAlive::No)
    }
}

fn build_default_payload(payload_id: PayloadId) -> BuiltPayload {
    let payload = SealedBlock::default();
    let fees = U256::ZERO;
    BuiltPayload::new(payload_id, payload, fees)
}
