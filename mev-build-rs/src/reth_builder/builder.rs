use crate::reth_builder::{
    build::*, cancelled::Cancelled, error::Error, payload_builder::*, types::*,
};
use ethereum_consensus::{
    clock::SystemClock,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, Slot},
    state_transition::Context,
};
use ethers::signers::{LocalWallet, Signer};
use mev_rs::{blinded_block_relayer::BlindedBlockRelayer, types::ProposerSchedule, Relay};
use reth_payload_builder::PayloadBuilderAttributes;
use reth_primitives::{Address, BlockNumberOrTag, Bytes, ChainSpec, B256, U256};
use reth_provider::{BlockReaderIdExt, BlockSource, StateProviderFactory};
use reth_transaction_pool::TransactionPool;
use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    ops::Deref,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream};

// The amount to let the build progress into its target slot.
// The build will stop early if stopped by an outside process.
const BUILD_DEADLINE_INTO_SLOT: Duration = Duration::from_millis(500);

// The frequency with which to try building a
// better payload in the context of one job.
const BUILD_PROGRESSION_INTERVAL: Duration = Duration::from_millis(500);

const GAS_BOUND_DIVISOR: u64 = 1024;

fn compute_preferred_gas_limit(preferred_gas_limit: u64, parent_gas_limit: u64) -> u64 {
    match preferred_gas_limit.cmp(&parent_gas_limit) {
        Ordering::Equal => preferred_gas_limit,
        Ordering::Greater => {
            let bound = parent_gas_limit + parent_gas_limit / GAS_BOUND_DIVISOR;
            preferred_gas_limit.min(bound - 1)
        }
        Ordering::Less => {
            let bound = parent_gas_limit - parent_gas_limit / GAS_BOUND_DIVISOR;
            preferred_gas_limit.max(bound + 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{compute_preferred_gas_limit, GAS_BOUND_DIVISOR};
    use std::cmp::Ordering;

    fn verify_limits(gas_limit: u64, parent_gas_limit: u64) -> bool {
        match gas_limit.cmp(&parent_gas_limit) {
            Ordering::Equal => true,
            Ordering::Greater => {
                let bound = parent_gas_limit + parent_gas_limit / GAS_BOUND_DIVISOR;
                gas_limit < bound
            }
            Ordering::Less => {
                let bound = parent_gas_limit - parent_gas_limit / GAS_BOUND_DIVISOR;
                gas_limit > bound
            }
        }
    }

    #[test]
    fn test_compute_preferred_gas_limit() {
        for t in &[
            // preferred, parent, computed
            (30_000_000, 30_000_000, 30_000_000),
            (30_029_000, 30_000_000, 30_029_000),
            (30_029_300, 30_000_000, 30_029_295),
            (29_970_710, 30_000_000, 29_970_710),
            (29_970_700, 30_000_000, 29_970_705),
        ] {
            assert_eq!(compute_preferred_gas_limit(t.0, t.1), t.2);
            assert!(verify_limits(t.2, t.1))
        }
    }
}

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

    relays: Vec<Relay>,

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
    // TODO: merge in `ProposerScheduler` here?
    proposer_schedule:
        BTreeMap<Slot, HashMap<BlsPublicKey, HashMap<ValidatorPreferences, Vec<RelayIndex>>>>,
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
        relays: Vec<Relay>,
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
            proposer_schedule: Default::default(),
            builds: Default::default(),
            cancels: Default::default(),
        };

        Self(Arc::new(Inner {
            secret_key,
            public_key,
            context,
            clock,
            relays,
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

    fn process_validator_schedule_for_relay(
        &self,
        relay: RelayIndex,
        schedule: &[ProposerSchedule],
    ) {
        // NOTE: we are trusting the data we get from a relay here;
        // this could conceivably be verified...
        let mut slots = Vec::with_capacity(schedule.len());
        let mut state = self.state.lock().unwrap();
        for duty in schedule {
            slots.push(duty.slot);
            let slot = state.proposer_schedule.entry(duty.slot).or_default();
            let registration = &duty.entry;
            let public_key = registration.message.public_key.clone();
            let preferences_by_slot = slot.entry(public_key).or_default();
            let preferences = registration.into();
            let registered_relays = preferences_by_slot.entry(preferences).or_default();
            if !registered_relays.contains(&relay) {
                // NOTE: given the API returns two epochs at a time, we can end up duplicating our
                // data so let's only add the relay if it is not already here
                registered_relays.push(relay);
            }
        }
        tracing::info!(?slots, %relay, "processed proposer schedule");
    }

    async fn on_epoch(&self, _epoch: Epoch) {
        // TODO: concurrent fetch
        for (index, relay) in self.relays.iter().enumerate() {
            match relay.get_proposal_schedule().await {
                Ok(schedule) => self.process_validator_schedule_for_relay(index, &schedule),
                Err(err) => {
                    tracing::warn!(err = %err, "error fetching proposer schedule from relay")
                }
            }
        }
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
        if let Some((earliest_slot, _)) = state.proposer_schedule.first_key_value() {
            for entry in *earliest_slot..slot {
                state.proposer_schedule.remove(&entry);
            }
        }
        state.builds.retain(|_, build| build.context.slot >= slot);
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
        for index in &context.relays {
            let relay = &self.relays[*index];
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
    NewBuild(BuildIdentifier),
    Duplicate(PayloadBuilderAttributes),
}

impl<Pool: TransactionPool, Client: StateProviderFactory + BlockReaderIdExt> Builder<Pool, Client> {
    // NOTE: this is held inside a lock currently, minimize work here
    fn construct_build_context(
        &self,
        slot: Slot,
        parent_hash: B256,
        proposer: &BlsPublicKey,
        payload_attributes: PayloadBuilderAttributes,
        validator_preferences: &ValidatorPreferences,
        relays: &[RelayIndex],
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

        let gas_limit =
            compute_preferred_gas_limit(validator_preferences.gas_limit, parent_block.gas_limit);
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
            proposer_fee_recipient: validator_preferences.fee_recipient.clone(),
            prev_randao: payload_attributes.prev_randao,
            withdrawals: payload_attributes.withdrawals,
            relays: relays.into(),
            chain_spec: self.chain_spec.clone(),
            block_env,
            cfg_env,
            extra_data: self.extra_data.clone(),
            builder_wallet: self.builder_wallet.clone(),
            // TODO: handle smart contract payments to fee recipient
            gas_reserve: 21000,
            bid_percent: self.bid_percent,
            subsidy: subsidy_in_wei,
        };
        Ok(context)
    }

    // Determine if a new build build should be created for the given context
    // fixed by `slot` and `payload_attributes`.
    // If a new build should be created, then do so and return the unique identifier
    // to the caller. If no new build should be created, `None` is returned.
    pub fn process_payload_attributes(
        &self,
        payload_attributes: PayloadBuilderAttributes,
    ) -> Result<PayloadAttributesProcessingOutcome, Error> {
        let slot = self
            .clock
            .slot_at_time(Duration::from_secs(payload_attributes.timestamp).as_nanos())
            .expect("past genesis");
        let mut state = self.state.lock().unwrap();
        let eligible_proposals = state
            .proposer_schedule
            .get(&slot)
            .ok_or_else(|| Error::NoRegisteredValidatorsForSlot(slot))?;

        // TODO: should defer to our own view of consensus:
        // currently, if there is more than one element in `eligible_proposals`
        // then there are multiple views across our relay set...
        // let's simplify the return type here by picking the "majority view"...
        let (proposer, preferences) = eligible_proposals
            .iter()
            .max_by(|(_, relay_set_a), (_, relay_set_b)| relay_set_a.len().cmp(&relay_set_b.len()))
            .ok_or_else(|| Error::NoRegisteredValidatorsForSlot(slot))?;
        // TODO: think about handling divergent relay views
        // similarly, let's just service the "majority" relays for now...
        let (validator_preferences, relays) = preferences
            .iter()
            .max_by(|(_, relay_set_a), (_, relay_set_b)| relay_set_a.len().cmp(&relay_set_b.len()))
            .ok_or_else(|| Error::NoRegisteredValidatorsForSlot(slot))?;

        let parent_hash = payload_attributes.parent;
        let build_identifier = compute_build_id(slot, parent_hash, proposer);

        if state.builds.contains_key(&build_identifier) {
            return Ok(PayloadAttributesProcessingOutcome::Duplicate(payload_attributes))
        }

        tracing::info!(slot, ?relays, %build_identifier, "constructing new build");

        let context = self.construct_build_context(
            slot,
            parent_hash,
            proposer,
            payload_attributes,
            validator_preferences,
            relays,
        )?;

        let build = Arc::new(Build::new(context));

        // TODO: encapsulate these details
        let current_value = build.value();
        let cancel = Cancelled::default();
        if let Ok(BuildOutcome::BetterOrEqual(payload_with_payments)) =
            build_payload(&build.context, current_value, &self.client, &self.pool, &cancel)
        {
            let mut state = build.state.lock().unwrap();
            state.payload_with_payments = payload_with_payments;
        }
        state.builds.insert(build_identifier.clone(), build);
        state.cancels.insert(build_identifier.clone(), cancel);
        Ok(PayloadAttributesProcessingOutcome::NewBuild(build_identifier))
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
                    let current_value = build.value();
                    match build_payload(&build.context, current_value, &self.client, &self.pool, &cancel) {
                        Ok(BuildOutcome::BetterOrEqual(payload_with_payments)) => {
                            let mut state = build.state.lock().unwrap();
                            state.payload_with_payments = payload_with_payments;
                        }
                        Ok(BuildOutcome::Worse { .. }) => continue,
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
