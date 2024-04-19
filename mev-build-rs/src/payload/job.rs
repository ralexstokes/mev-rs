use crate::utils::payload_job::{PayloadTaskGuard, PendingPayload, ResolveBestPayload};
use futures_util::{Future, FutureExt};
use reth::{
    api::BuiltPayload,
    payload::{self, database::CachedReads, error::PayloadBuilderError, KeepPayloadJobAlive},
    providers::StateProviderFactory,
    tasks::TaskSpawner,
    transaction_pool::TransactionPool,
};
use reth_basic_payload_builder::{
    BuildArguments, BuildOutcome, Cancelled, PayloadBuilder, PayloadConfig,
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    sync::oneshot,
    time::{Interval, Sleep},
};
use tracing::{debug, trace};

pub struct PayloadJob<Client, Pool, Tasks, Builder>
where
    Builder: PayloadBuilder<Pool, Client>,
{
    pub config: PayloadConfig<Builder::Attributes>,
    pub client: Client,
    pub pool: Pool,
    pub executor: Tasks,
    pub deadline: Pin<Box<Sleep>>,
    pub interval: Interval,
    pub best_payload: Option<Builder::BuiltPayload>,
    pub pending_block: Option<PendingPayload<Builder::BuiltPayload>>,
    pub payload_task_guard: PayloadTaskGuard,
    pub cached_reads: Option<CachedReads>,
    pub builder: Builder,
}

impl<Client, Pool, Tasks, Builder> payload::PayloadJob for PayloadJob<Client, Pool, Tasks, Builder>
where
    Client: StateProviderFactory + Clone + Unpin + 'static,
    Pool: TransactionPool + Unpin + 'static,
    Tasks: TaskSpawner + Clone + 'static,
    Builder: PayloadBuilder<Pool, Client> + Unpin + 'static,
    <Builder as PayloadBuilder<Pool, Client>>::Attributes: Unpin + Clone,
    <Builder as PayloadBuilder<Pool, Client>>::BuiltPayload: Unpin + Clone,
{
    type PayloadAttributes = Builder::Attributes;
    type ResolvePayloadFuture = ResolveBestPayload<Self::BuiltPayload>;
    type BuiltPayload = Builder::BuiltPayload;

    fn best_payload(&self) -> Result<Self::BuiltPayload, PayloadBuilderError> {
        if let Some(ref payload) = self.best_payload {
            return Ok(payload.clone())
        }
        // No payload has been built yet, but we need to return something that the CL then can
        // deliver, so we need to return an empty payload.
        //
        // Note: it is assumed that this is unlikely to happen, as the payload job is started right
        // away and the first full block should have been built by the time CL is requesting the
        // payload.
        Builder::build_empty_payload(&self.client, self.config.clone())
    }

    fn payload_attributes(&self) -> Result<Self::PayloadAttributes, PayloadBuilderError> {
        Ok(self.config.attributes.clone())
    }

    fn resolve(&mut self) -> (Self::ResolvePayloadFuture, KeepPayloadJobAlive) {
        let best_payload = self.best_payload.take();
        let maybe_better = self.pending_block.take();
        let mut empty_payload = None;

        if best_payload.is_none() {
            debug!(target: "payload_builder", id=%self.config.payload_id(), "no best payload yet to resolve, building empty payload");

            // let args = BuildArguments {
            //     client: self.client.clone(),
            //     pool: self.pool.clone(),
            //     cached_reads: self.cached_reads.take().unwrap_or_default(),
            //     config: self.config.clone(),
            //     cancel: Cancelled::default(),
            //     best_payload: None,
            // };

            // // TODO: create optimism payload job, that wraps this type, that implements
            // PayloadJob // with this branch. remove this branch from the non-op code.
            // remove // `on_missing_payload` requirement from builder trait
            // if let Some(payload) = self.builder.on_missing_payload(args) {
            //     debug!(target: "payload_builder", id=%self.config.payload_id(), "resolving
            // fallback payload as best payload");     return (
            //         ResolveBestPayload { best_payload: Some(payload), maybe_better, empty_payload
            // },         KeepPayloadJobAlive::Yes,
            //     )
            // }

            // if no payload has been built yet
            // no payload built yet, so we need to return an empty payload
            let (tx, rx) = oneshot::channel();
            let client = self.client.clone();
            let config = self.config.clone();
            self.executor.spawn_blocking(Box::pin(async move {
                let res = Builder::build_empty_payload(&client, config);
                let _ = tx.send(res);
            }));

            empty_payload = Some(rx);
        }

        let fut = ResolveBestPayload { best_payload, maybe_better, empty_payload };

        (fut, KeepPayloadJobAlive::No)
    }
}

impl<Client, Pool, Tasks, Builder> Future for PayloadJob<Client, Pool, Tasks, Builder>
where
    Client: StateProviderFactory + Clone + Unpin + 'static,
    Pool: TransactionPool + Unpin + 'static,
    Tasks: TaskSpawner + Clone + 'static,
    Builder: PayloadBuilder<Pool, Client> + Unpin + 'static,
    <Builder as PayloadBuilder<Pool, Client>>::Attributes: Unpin + Clone,
    <Builder as PayloadBuilder<Pool, Client>>::BuiltPayload: Unpin + Clone,
{
    type Output = Result<(), PayloadBuilderError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // check if the deadline is reached
        if this.deadline.as_mut().poll(cx).is_ready() {
            trace!(target: "payload_builder", "payload building deadline reached");
            return Poll::Ready(Ok(()))
        }

        // check if the interval is reached
        while this.interval.poll_tick(cx).is_ready() {
            // start a new job if there is no pending block and we haven't reached the deadline
            if this.pending_block.is_none() {
                trace!(target: "payload_builder", "spawn new payload build task");
                let (tx, rx) = oneshot::channel();
                let client = this.client.clone();
                let pool = this.pool.clone();
                let cancel = Cancelled::default();
                let _cancel = cancel.clone();
                let guard = this.payload_task_guard.clone();
                let payload_config = this.config.clone();
                let best_payload = this.best_payload.clone();
                let cached_reads = this.cached_reads.take().unwrap_or_default();
                let builder = this.builder.clone();
                this.executor.spawn_blocking(Box::pin(async move {
                    // acquire the permit for executing the task
                    let _permit = guard.0.acquire().await;
                    let args = BuildArguments {
                        client,
                        pool,
                        cached_reads,
                        config: payload_config,
                        cancel,
                        best_payload,
                    };
                    let result = builder.try_build(args);
                    let _ = tx.send(result);
                }));

                this.pending_block = Some(PendingPayload { _cancel, payload: rx });
            }
        }

        // poll the pending block
        if let Some(mut fut) = this.pending_block.take() {
            match fut.poll_unpin(cx) {
                Poll::Ready(Ok(outcome)) => {
                    this.interval.reset();
                    match outcome {
                        BuildOutcome::Better { payload, cached_reads } => {
                            this.cached_reads = Some(cached_reads);
                            debug!(target: "payload_builder", value = %payload.fees(), "built better payload");
                            let payload = payload;
                            this.best_payload = Some(payload);
                        }
                        BuildOutcome::Aborted { fees, cached_reads } => {
                            this.cached_reads = Some(cached_reads);
                            trace!(target: "payload_builder", worse_fees = %fees, "skipped payload build of worse block");
                        }
                        BuildOutcome::Cancelled => {
                            unreachable!("the cancel signal never fired")
                        }
                    }
                }
                Poll::Ready(Err(error)) => {
                    // job failed, but we simply try again next interval
                    debug!(target: "payload_builder", %error, "payload build attempt failed");
                }
                Poll::Pending => {
                    this.pending_block = Some(fut);
                }
            }
        }

        Poll::Pending
    }
}
