use crate::payload::{attributes::BuilderPayloadBuilderAttributes, builder::PayloadBuilder};
use futures_util::{Future, FutureExt};
use reth::{
    api::PayloadBuilderAttributes,
    payload::{
        self, database::CachedReads, error::PayloadBuilderError, EthBuiltPayload,
        KeepPayloadJobAlive,
    },
    primitives::{Address, B256, U256},
    providers::StateProviderFactory,
    revm::primitives::{BlockEnv, CfgEnvWithHandlerCfg},
    tasks::TaskSpawner,
    transaction_pool::TransactionPool,
};
use reth_basic_payload_builder::{
    BuildArguments, BuildOutcome, Cancelled, PayloadBuilder as _, PayloadConfig, PayloadTaskGuard,
    PendingPayload, ResolveBestPayload,
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::{
    sync::oneshot::{self, error::RecvError},
    time::{Interval, Sleep},
};
use tracing::{debug, error, trace, warn};

#[derive(Debug)]
pub struct PayloadFinalizerConfig {
    pub proposer_fee_recipient: Address,
    pub parent_hash: B256,
    // TODO: store with payload builder?
    pub cfg_env: CfgEnvWithHandlerCfg,
    // TODO: store with payload builder?
    pub block_env: BlockEnv,
}

pub struct PayloadJob<Client, Pool, Tasks> {
    pub config: PayloadConfig<BuilderPayloadBuilderAttributes>,
    pub client: Client,
    pub pool: Pool,
    pub executor: Tasks,
    pub deadline: Pin<Box<Sleep>>,
    pub interval: Interval,
    pub best_payload: Option<EthBuiltPayload>,
    pub pending_block: Option<PendingPayload<EthBuiltPayload>>,
    pub payload_task_guard: PayloadTaskGuard,
    pub cached_reads: Option<CachedReads>,
    // TODO: consider moving shared state here, rather than builder
    pub builder: PayloadBuilder,
    pub pending_bid_update: Option<BidUpdate>,
}

impl<Client, Pool, Tasks> payload::PayloadJob for PayloadJob<Client, Pool, Tasks>
where
    Client: StateProviderFactory + Clone + Unpin + 'static,
    Pool: TransactionPool + Unpin + 'static,
    Tasks: TaskSpawner + Clone + 'static,
{
    type PayloadAttributes = BuilderPayloadBuilderAttributes;
    type ResolvePayloadFuture = ResolveBestPayload<EthBuiltPayload>;
    type BuiltPayload = EthBuiltPayload;

    // TODO: do we need to customize this? if not, use default impl in some way
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
        <PayloadBuilder as reth_basic_payload_builder::PayloadBuilder<Pool, Client>>::build_empty_payload(&self.client, self.config.clone())
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
                let res = <PayloadBuilder as reth_basic_payload_builder::PayloadBuilder<
                    Pool,
                    Client,
                >>::build_empty_payload(&client, config);
                let _ = tx.send(res);
            }));

            empty_payload = Some(rx);
        }

        let fut = ResolveBestPayload { best_payload, maybe_better, empty_payload };

        (fut, KeepPayloadJobAlive::No)
    }
}

impl<Client, Pool, Tasks> Future for PayloadJob<Client, Pool, Tasks>
where
    Client: StateProviderFactory + Clone + Unpin + 'static,
    Pool: TransactionPool + Unpin + 'static,
    Tasks: TaskSpawner + Clone + 'static,
{
    type Output = Result<(), PayloadBuilderError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // poll for pending bids
        // NOTE: this should happen before anything else to ensure synchronization
        // invariants the bidding task relies on
        let mut pending_bid = false;
        if let Some(mut fut) = this.pending_bid_update.take() {
            pending_bid = true;
            match fut.poll_unpin(cx) {
                Poll::Pending => {
                    this.pending_bid_update = Some(fut);
                }
                Poll::Ready(Ok(maybe_dispatch)) => {
                    pending_bid = false;
                    if let Some((payload, value_to_bid)) = maybe_dispatch {
                        // TODO: handle the pending block, esp if this is the last bid
                        if let Some(proposal) = this.config.attributes.proposal.as_ref() {
                            let config = PayloadFinalizerConfig {
                                proposer_fee_recipient: proposal.proposer_fee_recipient,
                                parent_hash: this.config.attributes.parent(),
                                cfg_env: this.config.initialized_cfg.clone(),
                                block_env: this.config.initialized_block_env.clone(),
                            };
                            let client = this.client.clone();
                            let builder = this.builder.clone();
                            this.executor.spawn_blocking(Box::pin(async move {
                                // TODO: - track proposer payment, revenue
                                builder
                                    .finalize_payload_and_dispatch(
                                        client,
                                        payload,
                                        value_to_bid,
                                        &config,
                                    )
                                    .await
                            }));
                        } else {
                            error!(?payload, "attempt to finalize payload for an auction that is missing proposal attributes");
                        }
                    }
                }
                // bidder has terminated, so we terminate this job
                Poll::Ready(Err(_)) => return Poll::Ready(Ok(())),
            }
        }

        // check if the deadline is reached
        if this.deadline.as_mut().poll(cx).is_ready() {
            trace!(target: "payload_builder", "payload building deadline reached");
            if pending_bid {
                // if we have reached the deadline, but still have a pending bid outstanding,
                // return `Pending` to keep the job alive until we can settle the final bid update.
                return Poll::Pending
            } else {
                return Poll::Ready(Ok(()))
            }
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
                    let _permit = guard.acquire().await;
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

                this.pending_block = Some(PendingPayload::new(_cancel, rx));
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
                            // TODO: consider reworking this code path...
                            // If it stays, then at least skip clone here...
                            this.best_payload = Some(payload.clone());

                            if let Some(proposal) = this.config.attributes.proposal.as_ref() {
                                let (value_tx, value_rx) = oneshot::channel();
                                let fees = payload.fees();
                                let bidder = proposal.bidder.clone();
                                this.executor.spawn(Box::pin(async move {
                                    if bidder.is_closed() {
                                        return
                                    }
                                    if bidder.send((fees, value_tx)).await.is_err() {
                                        warn!("could not send fees to bidder");
                                    }
                                }));
                                this.pending_bid_update =
                                    Some(BidUpdate { value_rx, payload: Some(payload) });
                            }
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

pub struct BidUpdate {
    value_rx: oneshot::Receiver<Option<U256>>,
    // TODO: consider payload store, to skip shuttling data around
    payload: Option<EthBuiltPayload>,
}

impl Future for BidUpdate {
    type Output = Result<Option<(EthBuiltPayload, U256)>, RecvError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        match this.value_rx.poll_unpin(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(maybe_value)) => {
                Poll::Ready(Ok(maybe_value
                    .map(|value| (this.payload.take().expect("only called once"), value))))
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err)),
        }
    }
}
