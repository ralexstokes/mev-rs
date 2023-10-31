use crate::reth_builder::{
    bidder::{Bid, Bidder},
    builder::{Builder, PayloadAttributesProcessingOutcome},
    error::Error as BuilderError,
};
use ethereum_consensus::{
    clock::{Clock, SystemTimeProvider},
    crypto::SecretKey,
    state_transition::Context,
};
use ethers::signers::{coins_bip39::English, MnemonicBuilder, Signer};
use futures::StreamExt;
use mev_rs::{relay::parse_relay_endpoints, Error, Relay};
use reth_primitives::{Bytes, ChainSpec};
use serde::Deserialize;
use std::{future::Future, pin::Pin, sync::Arc, task::Poll};
use tokio::task::{JoinError, JoinHandle};
use tracing::{error, info};

const DEFAULT_BID_PERCENT: f64 = 0.9;

#[derive(Deserialize, Debug, Default, Clone)]
pub struct Config {
    pub secret_key: SecretKey,
    pub relays: Vec<String>,
    pub extra_data: Bytes,
    pub execution_mnemonic: String,
    // amount in milliseconds
    pub bidding_deadline_ms: u64,
    // amount to bid as a fraction of the block's value
    pub bid_percent: Option<f64>,
    // amount to add from the builder's wallet as a subsidy to the auction bid
    pub subsidy_gwei: Option<u64>,
}

pub struct Service<Pool, Client, Bidder> {
    builder: Builder<Pool, Client>,
    clock: Clock<SystemTimeProvider>,
    bidder: Arc<Bidder>,
}

impl<
        Pool: reth_transaction_pool::TransactionPool + 'static,
        Client: reth_provider::StateProviderFactory + reth_provider::BlockReaderIdExt + Clone + 'static,
        B: Bidder + Send + Sync + 'static,
    > Service<Pool, Client, B>
{
    pub fn from(
        config: &Config,
        context: Arc<Context>,
        clock: Clock<SystemTimeProvider>,
        pool: Pool,
        client: Client,
        bidder: Arc<B>,
        chain_spec: Arc<ChainSpec>,
    ) -> Result<(Self, Builder<Pool, Client>), Error> {
        let secret_key = &config.secret_key;
        let relays = parse_relay_endpoints(&config.relays)
            .into_iter()
            .map(|endpoint| Arc::new(Relay::from(endpoint)))
            .collect::<Vec<_>>();

        if relays.is_empty() {
            error!("no valid relays provided; please restart with correct configuration");
        } else {
            let count = relays.len();
            info!("configured with {count} relay(s)");
            for relay in &relays {
                info!(%relay, "configured with relay");
            }
        }

        let mut derivation_index = 0;
        let phrase = if let Some((phrase, index_str)) = config.execution_mnemonic.split_once(':') {
            derivation_index = index_str.parse::<u32>().expect("is valid");
            phrase
        } else {
            &config.execution_mnemonic
        };
        let wallet = MnemonicBuilder::<English>::default()
            .phrase(phrase)
            .index(derivation_index)
            .unwrap()
            .build()
            .expect("is valid phrase");
        let builder_wallet = wallet.with_chain_id(chain_spec.chain.id());

        let builder = Builder::new(
            secret_key.clone(),
            context.clone(),
            clock.clone(),
            relays,
            pool,
            client,
            chain_spec,
            config.extra_data.clone(),
            builder_wallet,
            config.bid_percent.unwrap_or(DEFAULT_BID_PERCENT),
            config.subsidy_gwei.unwrap_or_default(),
        );
        Ok((Service { builder: builder.clone(), clock, bidder }, builder))
    }

    pub async fn spawn(self) -> Result<ServiceHandle, Error> {
        let Self { builder, clock, bidder } = self;

        if clock.before_genesis() {
            let genesis = clock.duration_until_next_slot();
            tracing::warn!(duration = ?genesis, "waiting until genesis");
            tokio::time::sleep(genesis).await;
        }

        let current_epoch = clock.current_epoch().unwrap();
        builder.initialize(current_epoch).await;

        let builder_handle = builder.clone();
        // NOTE: validator management
        let clock_handle = clock.clone();
        let clock = tokio::spawn(async move {
            let builder = builder_handle;
            let slots = clock.stream_slots();

            tokio::pin!(slots);

            while let Some(slot) = slots.next().await {
                builder.on_slot(slot).await;
            }
        });

        let builder_handle = builder.clone();
        let bidder = tokio::spawn(async move {
            let builder = builder_handle;
            let builds = match builder.stream_builds() {
                Ok(stream) => stream,
                Err(err) => {
                    tracing::error!(err = ?err, "could not open builds stream");
                    return
                }
            };

            tokio::pin!(builds);

            while let Some(id) = builds.next().await {
                let build = builder.build_for(&id).expect("only asking for existing builds");

                // TODO: constrain bidders to finite lifetime
                let builder = builder.clone();
                let bidder = bidder.clone();
                tokio::task::spawn_blocking(move || {
                    tokio::runtime::Handle::current().block_on(Box::pin(async move {
                        loop {
                            match bidder.bid_for(&build).await {
                                Ok(Some(bid)) => {
                                    if let Err(err) = builder.submit_bid(&id).await {
                                        tracing::warn!(id = %id, slot=?build.context.slot, err = %err, "error submitting bid for build");
                                    }
                                    if matches!(bid, Bid::Done) {
                                        builder.cancel_build(&id);
                                        break;
                                    }
                                }
                                Ok(None) => continue,
                                Err(err) => {
                                    tracing::warn!(id = %id, err = %err, "error determining bid for build");
                                }
                            }
                        }
                    }));
                });
            }
        });

        let payload_builder = tokio::spawn(async move {
            let payload_attributes = match builder.stream_payload_attributes() {
                Ok(stream) => stream,
                Err(err) => {
                    tracing::error!(err = ?err, "could not open payload attributes stream");
                    return
                }
            };

            tokio::pin!(payload_attributes);

            while let Some(attrs) = payload_attributes.next().await {
                let slot = clock_handle
                    .slot_at_time(std::time::Duration::from_secs(attrs.timestamp).as_nanos())
                    .expect("after genesis");
                tracing::trace!(id = %attrs.payload_id(), slot, "got attrs from CL");
                match builder.process_payload_attributes(attrs) {
                    Ok(PayloadAttributesProcessingOutcome::NewBuilds(new_builds)) => {
                        for id in new_builds {
                            let builder = builder.clone();
                            tokio::task::spawn_blocking(move || {
                                tokio::runtime::Handle::current().block_on(Box::pin(async move {
                                if let Err(err) = builder.start_build(&id).await {
                                    tracing::warn!(id = %id, err = ?err, "failed to start build");
                                }
                            }));
                            });
                        }
                    }
                    Ok(PayloadAttributesProcessingOutcome::Duplicate(_)) => continue,
                    Err(BuilderError::NoProposals(_)) => continue,
                    Err(err) => {
                        tracing::warn!(err = ?err, "could not process payload attributes");
                    }
                }
            }
        });

        Ok(ServiceHandle { clock, payload_builder, bidder })
    }
}

#[pin_project::pin_project]
pub struct ServiceHandle {
    #[pin]
    clock: JoinHandle<()>,
    #[pin]
    payload_builder: JoinHandle<()>,
    #[pin]
    bidder: JoinHandle<()>,
}

impl Future for ServiceHandle {
    type Output = Result<(), JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let clock = this.clock.poll(cx);
        if clock.is_ready() {
            return clock
        }
        let builder = this.payload_builder.poll(cx);
        if builder.is_ready() {
            return builder
        }
        this.bidder.poll(cx)
    }
}
