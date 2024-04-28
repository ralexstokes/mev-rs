use futures_util::{ready, FutureExt};
use reth::payload::error::PayloadBuilderError;
use reth_basic_payload_builder::{BuildOutcome, Cancelled};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::{oneshot, Semaphore};
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub struct PayloadTaskGuard(pub Arc<Semaphore>);

impl PayloadTaskGuard {
    pub fn new(max_payload_tasks: usize) -> Self {
        Self(Arc::new(Semaphore::new(max_payload_tasks)))
    }
}

#[derive(Debug)]
pub struct PendingPayload<P> {
    pub _cancel: Cancelled,
    pub payload: oneshot::Receiver<Result<BuildOutcome<P>, PayloadBuilderError>>,
}

impl<P> Future for PendingPayload<P> {
    type Output = Result<BuildOutcome<P>, PayloadBuilderError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let res = ready!(self.payload.poll_unpin(cx));
        Poll::Ready(res.map_err(Into::into).and_then(|res| res))
    }
}

#[derive(Debug)]
pub struct ResolveBestPayload<Payload> {
    pub best_payload: Option<Payload>,
    pub maybe_better: Option<PendingPayload<Payload>>,
    pub empty_payload: Option<oneshot::Receiver<Result<Payload, PayloadBuilderError>>>,
}

impl<Payload> Future for ResolveBestPayload<Payload>
where
    Payload: Unpin,
{
    type Output = Result<Payload, PayloadBuilderError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        // check if there is a better payload before returning the best payload
        if let Some(fut) = Pin::new(&mut this.maybe_better).as_pin_mut() {
            if let Poll::Ready(res) = fut.poll(cx) {
                this.maybe_better = None;
                if let Ok(BuildOutcome::Better { payload, .. }) = res {
                    debug!(target: "payload_builder", "resolving better payload");
                    return Poll::Ready(Ok(payload))
                }
            }
        }

        if let Some(best) = this.best_payload.take() {
            debug!(target: "payload_builder", "resolving best payload");
            return Poll::Ready(Ok(best))
        }

        let mut empty_payload = this.empty_payload.take().expect("polled after completion");
        match empty_payload.poll_unpin(cx) {
            Poll::Ready(Ok(res)) => {
                if let Err(err) = &res {
                    warn!(target: "payload_builder", %err, "failed to resolve empty payload");
                } else {
                    debug!(target: "payload_builder", "resolving empty payload");
                }
                Poll::Ready(res)
            }
            Poll::Ready(Err(err)) => Poll::Ready(Err(err.into())),
            Poll::Pending => {
                this.empty_payload = Some(empty_payload);
                Poll::Pending
            }
        }
    }
}
