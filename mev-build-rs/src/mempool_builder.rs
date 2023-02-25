use async_trait::async_trait;
use beacon_api_client::{BeaconProposerRegistration, Client, ProposerDuty};
use ethereum_consensus::{
    clock::{convert_timestamp_to_slot, get_current_unix_time_in_secs},
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, ExecutionAddress, Hash32, Root, Slot},
    state_transition::Context,
};
use mev_rs::{
    engine_api_proxy::{client::Client as EngineApiClient, server::Proxy, types::BuildJob},
    types::{
        BidRequest, BuilderBid, ExecutionPayload, ExecutionPayloadHeader, SignedBlindedBeaconBlock,
        SignedBuilderBid, SignedValidatorRegistration,
    },
    BlindedBlockProvider, Error, ProposerScheduler, ValidatorRegistry,
};
use parking_lot::Mutex;
use std::{collections::HashMap, ops::Deref, sync::Arc};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone)]
pub struct Builder(Arc<Inner>);

impl Deref for Builder {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    secret_key: SecretKey,
    public_key: BlsPublicKey,
    genesis_validators_root: Root,
    validator_registry: ValidatorRegistry,
    proposer_scheduler: ProposerScheduler,
    engine_api_client: EngineApiClient,
    proxy: Arc<Proxy>,
    context: Arc<Context>,
    state: Mutex<State>,
}

#[derive(Default, Debug, Hash, PartialEq, Eq)]
struct Coordinate {
    slot: Slot,
    parent_hash: Hash32,
}

#[derive(Default, Debug)]
struct State {
    did_update_validator_registry: bool,
    build_jobs: HashMap<Coordinate, BuildJob>,
    payloads: HashMap<BidRequest, ExecutionPayload>,
}

impl Builder {
    pub fn new(
        secret_key: SecretKey,
        genesis_validators_root: Root,
        client: Client,
        context: Arc<Context>,
        engine_api_client: EngineApiClient,
        proxy: Arc<Proxy>,
    ) -> Self {
        let public_key = secret_key.public_key();
        let validator_registry = ValidatorRegistry::new(client.clone());
        let proposer_scheduler = ProposerScheduler::new(client);

        Self(Arc::new(Inner {
            secret_key,
            public_key,
            genesis_validators_root,
            validator_registry,
            proposer_scheduler,
            engine_api_client,
            proxy,
            context,
            state: Default::default(),
        }))
    }

    pub async fn process_duties(&self, duties: &[ProposerDuty]) -> Result<(), Error> {
        let mut preparations = vec![];
        for duty in duties {
            if let Some(preferences) = self.validator_registry.get_preferences(&duty.public_key) {
                let public_key = &preferences.public_key;
                match self.validator_registry.get_validator_index(public_key) {
                    Some(validator_index) => {
                        let preparation = BeaconProposerRegistration {
                            validator_index,
                            fee_recipient: preferences.fee_recipient,
                        };
                        preparations.push(preparation);
                    }
                    None => {
                        tracing::warn!("could not find index for public key {public_key}")
                    }
                }
            }
        }
        self.proposer_scheduler.dispatch_proposer_preparations(&preparations).await?;
        Ok(())
    }

    pub async fn poll_proposer_duties(&self, epoch: Epoch) {
        match self.proposer_scheduler.fetch_duties(epoch).await {
            Ok(duties) => {
                if let Err(err) = self.process_duties(&duties).await {
                    tracing::warn!("could not process duties in epoch {epoch}: {err}");
                }
            }
            Err(err) => tracing::warn!("could not load proposer duties in epoch {epoch}: {err}"),
        }
    }

    async fn on_epoch(&self, epoch: Epoch) {
        // TODO: only get on changes to set
        if let Err(err) = self.validator_registry.load().await {
            tracing::warn!("could not load validator set in epoch {epoch}: {err}")
        }
        self.poll_proposer_duties(epoch).await;
        self.poll_proposer_duties(epoch + 1).await;
    }

    pub async fn initialize(&self, current_epoch: Epoch) {
        self.on_epoch(current_epoch).await;

        let public_key = &self.public_key;
        tracing::info!("builder initialized with public key {public_key}");
    }

    pub async fn on_slot(&self, slot: Slot) {
        let next_epoch = slot % self.context.slots_per_epoch == 0;
        let did_update_validator_registry = {
            let mut state = self.state.lock();
            let did_update = state.did_update_validator_registry;
            state.did_update_validator_registry = false;
            did_update
        };
        if next_epoch || did_update_validator_registry {
            let epoch = slot / self.context.slots_per_epoch;
            self.on_epoch(epoch).await;
        }
        let mut state = self.state.lock();
        // TODO better windowing on build job garbage collecting
        state.build_jobs.retain(|coordinate, _| coordinate.slot >= slot - 3);
        state.payloads.retain(|bid_request, _| bid_request.slot >= slot - 3);
    }

    pub fn process_build_job(
        &self,
        job @ BuildJob { head_block_hash, timestamp, .. }: &BuildJob,
    ) -> Result<(), Error> {
        let parent_hash = head_block_hash.clone();
        let genesis_time = self
            .context
            .genesis_time()
            // TODO update method on Context
            .unwrap_or(self.context.min_genesis_time + self.context.genesis_delay);
        let slot =
            convert_timestamp_to_slot(*timestamp, genesis_time, self.context.seconds_per_slot);
        let coordinate = Coordinate { slot, parent_hash };
        let mut state = self.state.lock();
        tracing::trace!("at {coordinate:?}, inserting build job from engine API: {job:?}");
        state.build_jobs.insert(coordinate, job.clone());
        Ok(())
    }

    pub fn spawn(self, mut build_jobs: mpsc::Receiver<BuildJob>) -> JoinHandle<()> {
        // TODO move "IO" to wrapping type
        tokio::spawn(async move {
            loop {
                match build_jobs.recv().await {
                    Some(job) => {
                        if let Err(err) = self.process_build_job(&job) {
                            tracing::warn!("could not process build job {job:?}: {err}")
                        }
                    }
                    None => return,
                }
            }
        })
    }
}

fn verify_job_for_proposer(
    validator_registry: &ValidatorRegistry,
    fee_recipient: &ExecutionAddress,
    public_key: &BlsPublicKey,
) -> Result<(), Error> {
    let preferences = validator_registry
        .get_preferences(public_key)
        .ok_or_else(|| Error::ValidatorNotRegistered(public_key.clone()))?;

    if &preferences.fee_recipient != fee_recipient {
        Err(Error::UnknownFeeRecipient(public_key.clone(), fee_recipient.clone()))
    } else {
        Ok(())
    }
}

#[async_trait]
impl BlindedBlockProvider for Builder {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let current_time = get_current_unix_time_in_secs();
        self.validator_registry.validate_registrations(
            registrations,
            current_time,
            &self.context,
        )?;
        // NOTE: TODO clean up flow here
        let mut state = self.state.lock();
        state.did_update_validator_registry = true;
        Ok(())
    }

    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error> {
        let build_job = {
            let coordinate =
                Coordinate { slot: bid_request.slot, parent_hash: bid_request.parent_hash.clone() };
            let mut state = self.state.lock();
            state
                .build_jobs
                .remove(&coordinate)
                .ok_or_else(|| Error::NoBidPrepared(Box::new(bid_request.clone())))?
        };
        verify_job_for_proposer(
            &self.validator_registry,
            &build_job.suggested_fee_recipient,
            &bid_request.public_key,
        )?;
        let payload_id = &build_job.payload_id;
        let auth_token = {
            let token = self.proxy.token.lock();
            token.clone()
        };
        let version = build_job.version;
        let (mut payload, value) =
            self.engine_api_client.get_payload_with_value(payload_id, &auth_token, version).await?;
        let header = ExecutionPayloadHeader::try_from(&mut payload)?;
        let mut state = self.state.lock();
        state.payloads.insert(bid_request.clone(), payload);

        let bid = BuilderBid::from((header, value, &self.public_key));
        let signed_bid = bid.sign(&self.secret_key, &self.context)?;
        Ok(signed_bid)
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let slot = signed_block.slot();
        let public_key = self.proposer_scheduler.get_proposer_for(slot)?;
        signed_block.verify_signature(&public_key, self.genesis_validators_root, &self.context)?;

        let parent_hash = signed_block.parent_hash();
        let bid_request = BidRequest { slot, parent_hash: parent_hash.clone(), public_key };
        let mut state = self.state.lock();
        state
            .payloads
            .remove(&bid_request)
            .ok_or_else(|| Error::MissingPayload(signed_block.block_hash().clone()))
    }
}
