use crate::reth_builder::{
    auction_schedule::AuctionSchedule, build::*, error::Error, payload_builder::*,
};
use ethereum_consensus::{
    clock::SystemClock,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, ExecutionAddress, Slot},
    state_transition::Context,
};
use ethers::signers::{LocalWallet, Signer};
use mev_rs::{blinded_block_relayer::BlindedBlockRelayer, compute_preferred_gas_limit, Relay};
use reth_basic_payload_builder::Cancelled;
use reth_payload_builder::PayloadBuilderAttributes;
use reth_primitives::{Address, BlockNumberOrTag, Bytes, ChainSpec, B256, U256};
use reth_provider::{BlockReaderIdExt, BlockSource, StateProviderFactory};
use reth_transaction_pool::TransactionPool;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream};
use tracing::debug;

// The amount to let the build progress into its target slot.
// The build will stop early if stopped by an outside process.
const BUILD_DEADLINE_INTO_SLOT: Duration = Duration::from_millis(500);

// The frequency with which to try building a
// better payload in the context of one job.
const BUILD_PROGRESSION_INTERVAL: Duration = Duration::from_millis(500);

/// `Builder` builds blocks for proposers registered to connected relays.
#[derive(Clone)]
pub struct Builder<Pool, Client>(Arc<Inner<Pool, Client>>);

impl<Pool, Client> Deref for Builder<Pool, Client> {
    type Target = Inner<Pool, Client>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner<Pool, Client> {
    secret_key: SecretKey,
    public_key: BlsPublicKey,

    context: Arc<Context>,
    clock: SystemClock,

    relays: Vec<Arc<Relay>>,
    auction_schedule: AuctionSchedule,

    pool: Pool,
    client: Client,
    chain_spec: Arc<ChainSpec>,
    extra_data: Bytes,
    builder_wallet: LocalWallet,
    bid_percent: f64,
    subsidy_gwei: u64,

    pub(crate) payload_attributes_tx: mpsc::Sender<PayloadBuilderAttributes>,
    builds_tx: mpsc::Sender<BuildIdentifier>,
    state: Mutex<State>,
}

#[derive(Default, Debug)]
struct State {
    payload_attributes_rx: Option<mpsc::Receiver<PayloadBuilderAttributes>>,
    builds_rx: Option<mpsc::Receiver<BuildIdentifier>>,
    builds: HashMap<BuildIdentifier, Arc<Build>>,
    // TODO: rework cancellation discipline here...
    cancels: HashMap<BuildIdentifier, Cancelled>,
}

impl<Pool, Client> Builder<Pool, Client> {
    // TODO: clean up argument set
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        secret_key: SecretKey,
        context: Arc<Context>,
        clock: SystemClock,
        relays: Vec<Arc<Relay>>,
        pool: Pool,
        client: Client,
        chain_spec: Arc<ChainSpec>,
        extra_data: Bytes,
        builder_wallet: LocalWallet,
        bid_percent: f64,
        subsidy_gwei: u64,
    ) -> Self {
        let public_key = secret_key.public_key();

        let (attrs_tx, attrs_rx) = mpsc::channel::<PayloadBuilderAttributes>(16);
        let (builds_tx, builds_rx) = mpsc::channel::<BuildIdentifier>(16);

        let state = State {
            payload_attributes_rx: Some(attrs_rx),
            builds_rx: Some(builds_rx),
            builds: Default::default(),
            cancels: Default::default(),
        };

        Self(Arc::new(Inner {
            secret_key,
            public_key,
            context,
            clock,
            relays,
            auction_schedule: Default::default(),
            pool,
            client,
            chain_spec,
            builder_wallet,
            bid_percent,
            subsidy_gwei,
            extra_data,
            payload_attributes_tx: attrs_tx,
            builds_tx,
            state: Mutex::new(state),
        }))
    }

    async fn on_epoch(&self, epoch: Epoch) {
        // TODO: concurrent fetch
        // TODO: batch updates to auction schedule
        for relay in self.relays.iter() {
            match relay.get_proposal_schedule().await {
                Ok(schedule) => {
                    let slots = self.auction_schedule.process(relay.clone(), &schedule);
                    tracing::info!(epoch, ?slots, %relay, "processed proposer schedule");
                }
                Err(err) => {
                    tracing::warn!(err = %err, "error fetching proposer schedule from relay")
                }
            }
        }
        let slot = epoch * self.context.slots_per_epoch;
        self.auction_schedule.clear(slot);
    }

    pub async fn initialize(&self, current_epoch: Epoch) {
        self.on_epoch(current_epoch).await;

        let public_key = &self.public_key;
        let relays = &self.relays;
        let address = self.builder_wallet.address();
        tracing::info!(%public_key, %address, ?relays, "builder initialized");
    }

    pub async fn on_slot(&self, slot: Slot) {
        tracing::info!(slot, "processing slot");
        let next_epoch = slot % self.context.slots_per_epoch == 0;
        if next_epoch {
            let epoch = slot / self.context.slots_per_epoch;
            tracing::info!(epoch, "processing epoch");
            self.on_epoch(epoch).await;
        }

        let mut state = self.state.lock().unwrap();
        state.builds.retain(|_, build| build.context.slot >= slot);
        let live_builds = state.builds.keys().cloned().collect::<HashSet<_>>();
        state.cancels.retain(|id, _| live_builds.contains(id));
    }

    pub fn stream_payload_attributes(
        &self,
    ) -> Result<impl Stream<Item = PayloadBuilderAttributes>, Error> {
        let mut state = self.state.lock().unwrap();
        let rx = state.payload_attributes_rx.take();
        if let Some(rx) = rx {
            Ok(ReceiverStream::new(rx))
        } else {
            Err(Error::Internal("can only yield payload attributes stream once"))
        }
    }

    pub fn stream_builds(&self) -> Result<impl Stream<Item = BuildIdentifier>, Error> {
        let mut state = self.state.lock().unwrap();
        let rx = state.builds_rx.take();
        if let Some(rx) = rx {
            Ok(ReceiverStream::new(rx))
        } else {
            Err(Error::Internal("can only yield builds stream once"))
        }
    }

    pub fn build_for(&self, id: &BuildIdentifier) -> Option<Arc<Build>> {
        self.state.lock().unwrap().builds.get(id).cloned()
    }

    fn cancel_for(&self, id: &BuildIdentifier) -> Option<Cancelled> {
        self.state.lock().unwrap().cancels.get(id).cloned()
    }

    pub fn cancel_build(&self, id: &BuildIdentifier) {
        self.state.lock().unwrap().cancels.remove(id);
    }

    pub async fn submit_bid(&self, id: &BuildIdentifier) -> Result<(), Error> {
        let build = self.build_for(id).ok_or_else(|| Error::MissingBuild(id.clone()))?;

        let context = &build.context;

        let (signed_submission, builder_payment) =
            build.prepare_bid(&self.secret_key, &self.public_key, &self.context)?;

        // TODO: make calls concurrently
        for relay in context.relays.iter() {
            let slot = signed_submission.message.slot;
            let parent_hash = &signed_submission.message.parent_hash;
            let block_hash = &signed_submission.message.block_hash;
            let value = &signed_submission.message.value;
            tracing::info!(%id, %relay, slot, %parent_hash, %block_hash, ?value, %builder_payment, "submitting bid");
            match relay.submit_bid(&signed_submission).await {
                Ok(_) => tracing::info!(%id, %relay, "successfully submitted bid"),
                Err(err) => {
                    tracing::warn!(%err, %id, %relay, "error submitting bid");
                }
            }
        }

        Ok(())
    }
}

pub enum PayloadAttributesProcessingOutcome {
    NewBuilds(Vec<BuildIdentifier>),
    Duplicate(PayloadBuilderAttributes),
}

impl<Pool: TransactionPool, Client: StateProviderFactory + BlockReaderIdExt + Clone>
    Builder<Pool, Client>
{
    // TODO: clean up argument set
    #[allow(clippy::too_many_arguments)]
    // NOTE: this is held inside a lock currently, minimize work here
    fn construct_build_context(
        &self,
        slot: Slot,
        parent_hash: B256,
        proposer: &BlsPublicKey,
        payload_attributes: &PayloadBuilderAttributes,
        proposer_fee_recipient: ExecutionAddress,
        preferred_gas_limit: u64,
        relays: HashSet<Arc<Relay>>,
    ) -> Result<BuildContext, Error> {
        let parent_block = if parent_hash.is_zero() {
            // use latest block if parent is zero: genesis block
            self.client
                .block_by_number_or_tag(BlockNumberOrTag::Latest)?
                .ok_or_else(|| Error::MissingParentBlock(payload_attributes.parent))?
                .seal_slow()
        } else {
            let block = self
                .client
                .find_block_by_hash(parent_hash, BlockSource::Any)?
                .ok_or_else(|| Error::MissingParentBlock(parent_hash))?;

            // we already know the hash, so we can seal it
            block.seal(parent_hash)
        };

        // configure evm env based on parent block
        let (cfg_env, mut block_env) =
            payload_attributes.cfg_and_block_env(&self.chain_spec, &parent_block);

        let gas_limit = compute_preferred_gas_limit(preferred_gas_limit, parent_block.gas_limit);
        block_env.gas_limit = U256::from(gas_limit);

        // TODO: configurable "fee collection strategy"
        // fee collection strategy: drive all fees to builder
        block_env.coinbase = Address::from(self.builder_wallet.address().to_fixed_bytes());

        let subsidy = U256::from(self.subsidy_gwei);
        let subsidy_in_wei = subsidy * U256::from(10u64.pow(9));
        let context = BuildContext {
            slot,
            parent_hash,
            proposer: proposer.clone(),
            timestamp: payload_attributes.timestamp,
            proposer_fee_recipient,
            prev_randao: payload_attributes.prev_randao,
            withdrawals: payload_attributes.withdrawals.clone(),
            relays: relays.into_iter().collect(),
            chain_spec: self.chain_spec.clone(),
            block_env,
            cfg_env,
            extra_data: self.extra_data.clone(),
            builder_wallet: self.builder_wallet.clone(),
            // TODO: handle smart contract payments to fee recipient
            _gas_reserve: 21000,
            bid_percent: self.bid_percent,
            subsidy: subsidy_in_wei,
            parent_block: Arc::new(parent_block),
            payload_attributes: payload_attributes.clone(),
        };
        Ok(context)
    }

    // Determine if a new build should be created for the given context fixed by `slot` and
    // `payload_attributes`. Outcome is returned to reflect any updates.
    pub fn process_payload_attributes(
        &self,
        payload_attributes: PayloadBuilderAttributes,
    ) -> Result<PayloadAttributesProcessingOutcome, Error> {
        let slot = self
            .clock
            .slot_at_time(Duration::from_secs(payload_attributes.timestamp).as_nanos())
            .expect("past genesis");
        let proposals =
            self.auction_schedule.take_proposal(slot).ok_or_else(|| Error::NoProposals(slot))?;

        let parent_hash = payload_attributes.parent;
        let mut state = self.state.lock().expect("can lock");
        let mut new_builds = vec![];
        for (proposer, relays) in proposals {
            let build_identifier = compute_build_id(slot, parent_hash, &proposer.public_key);

            if state.builds.contains_key(&build_identifier) {
                return Ok(PayloadAttributesProcessingOutcome::Duplicate(payload_attributes))
            }

            tracing::info!(slot, ?relays, %build_identifier, "constructing new build");

            let context = self.construct_build_context(
                slot,
                parent_hash,
                &proposer.public_key,
                &payload_attributes,
                proposer.fee_recipient,
                proposer.gas_limit,
                relays,
            )?;

            let build = Arc::new(Build::new(context));

            // TODO: encapsulate these details
            let cancel = Cancelled::default();
            if let Ok(BuildOutcome::BetterOrEqual(payload_with_payments)) = build_payload(
                &build.context,
                None,
                self.client.clone(),
                self.pool.clone(),
                cancel.clone(),
            ) {
                let mut state = build.state.lock().unwrap();
                state.payload_with_payments = payload_with_payments;
            }
            state.builds.insert(build_identifier.clone(), build);
            state.cancels.insert(build_identifier.clone(), cancel);
            new_builds.push(build_identifier);
        }

        Ok(PayloadAttributesProcessingOutcome::NewBuilds(new_builds))
    }

    // Drives the build referenced by `id`. Inside a context where blocking is ok.
    pub async fn start_build(&self, id: &BuildIdentifier) -> Result<(), Error> {
        let build = self.build_for(id).ok_or_else(|| Error::MissingBuild(id.clone()))?;
        if self.builds_tx.send(id.clone()).await.is_err() {
            tracing::warn!(%id, "could not send build to stream of builds, listeners will ignore");
        }

        let deadline = self.clock.duration_until_slot(build.context.slot);
        let deadline = tokio::time::sleep(deadline + BUILD_DEADLINE_INTO_SLOT);
        tokio::pin!(deadline);

        let mut interval = tokio::time::interval(BUILD_PROGRESSION_INTERVAL);

        let cancel = self.cancel_for(id).ok_or_else(|| Error::MissingBuild(id.clone()))?;

        loop {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    // NOTE: catch here as well, otherwise we get issues with
                    // leaking contexts beyond the top-level
                    // TODO: implement graceful shutdowns
                    tracing::trace!(%id, "aborting build due to signal");
                    return Ok(())
                }
                () = &mut deadline => {
                    tracing::trace!(%id, slot = build.context.slot, "deadline for build reached");
                    return Ok(())
                }
                _ = interval.tick() => {
                    match build_payload(&build.context, build.payload(), self.client.clone(), self.pool.clone(), cancel.clone()) {
                        Ok(BuildOutcome::BetterOrEqual(payload_with_payments)) => {
                            let mut state = build.state.lock().unwrap();
                            state.payload_with_payments = payload_with_payments;
                        }
                        Ok(BuildOutcome::Worse { threshold, provided  }) => {
                           debug!(%threshold, %provided, "did not build a better payload");
                        }
                        Ok(BuildOutcome::Cancelled) => {
                            tracing::trace!(%id, "build cancelled");
                            return Ok(())
                        }
                        Err(err) => tracing::warn!(%err, "error building payload"),
                    }
                }
            }
        }
    }
}
