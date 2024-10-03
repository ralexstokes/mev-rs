use crate::auction_context::AuctionContext;
use async_trait::async_trait;
use beacon_api_client::{BroadcastValidation, PayloadAttributesEvent, SubmitSignedBeaconBlock};
use ethereum_consensus::{
    clock::{duration_since_unix_epoch, get_current_unix_time_in_nanos},
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, Root, Slot, U256},
    ssz::prelude::HashTreeRoot,
    state_transition::Context,
    Error as ConsensusError, Fork,
};
use mev_rs::{
    blinded_block_relayer::{BlockSubmissionFilter, DeliveredPayloadFilter},
    signing::{compute_consensus_domain, verify_signed_builder_data, verify_signed_data},
    types::{
        block_submission::data_api::{PayloadTrace, SubmissionTrace},
        AuctionContents, AuctionRequest, BidTrace, ExecutionPayload, ExecutionPayloadHeader,
        ProposerSchedule, SignedBidSubmission, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
    BlindedBlockDataProvider, BlindedBlockProvider, BlindedBlockRelayer, Error, ProposerScheduler,
    RelayError, ValidatorRegistry,
};
use parking_lot::Mutex;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
    time::Duration,
};
use tracing::{debug, error, info, trace, warn};

#[cfg(not(feature = "minimal-preset"))]
use beacon_api_client::mainnet::Client as ApiClient;
#[cfg(feature = "minimal-preset")]
use beacon_api_client::minimal::Client as ApiClient;
#[cfg(not(feature = "minimal-preset"))]
use ethereum_consensus::{
    bellatrix::mainnet as bellatrix,
    capella::mainnet as capella,
    deneb::mainnet as deneb,
    types::mainnet::{ExecutionPayloadHeaderRef, SignedBeaconBlock},
};
#[cfg(feature = "minimal-preset")]
use ethereum_consensus::{
    bellatrix::minimal as bellatrix,
    capella::minimal as capella,
    deneb::minimal as deneb,
    types::minimal::{ExecutionPayloadHeaderRef, SignedBeaconBlock},
};

// Sets the lifetime of an auction with respect to its proposal slot.
const AUCTION_LIFETIME_SLOTS: Slot = 1;
const HISTORY_LOOK_BEHIND_EPOCHS: Epoch = 4;

fn validate_header_equality(
    local_header: &ExecutionPayloadHeader,
    provided_header: ExecutionPayloadHeaderRef<'_>,
) -> Result<(), RelayError> {
    match local_header {
        ExecutionPayloadHeader::Bellatrix(local_header) => {
            let provided_header =
                provided_header.bellatrix().ok_or(RelayError::InvalidExecutionPayloadInBlock)?;
            if local_header != provided_header {
                return Err(RelayError::InvalidExecutionPayloadInBlock);
            }
        }
        ExecutionPayloadHeader::Capella(local_header) => {
            let provided_header =
                provided_header.capella().ok_or(RelayError::InvalidExecutionPayloadInBlock)?;
            if local_header != provided_header {
                return Err(RelayError::InvalidExecutionPayloadInBlock);
            }
        }
        ExecutionPayloadHeader::Deneb(local_header) => {
            let provided_header =
                provided_header.deneb().ok_or(RelayError::InvalidExecutionPayloadInBlock)?;
            if local_header != provided_header {
                return Err(RelayError::InvalidExecutionPayloadInBlock);
            }
        }
    }
    Ok(())
}

fn unblind_block(
    signed_blinded_beacon_block: &SignedBlindedBeaconBlock,
    execution_payload: &ExecutionPayload,
) -> Result<SignedBeaconBlock, Error> {
    match signed_blinded_beacon_block {
        SignedBlindedBeaconBlock::Bellatrix(blinded_block) => {
            let signature = blinded_block.signature.clone();
            let block = &blinded_block.message;
            let body = &block.body;
            let execution_payload = execution_payload.bellatrix().ok_or(Error::InvalidFork {
                expected: Fork::Bellatrix,
                provided: execution_payload.version(),
            })?;

            let inner = bellatrix::SignedBeaconBlock {
                message: bellatrix::BeaconBlock {
                    slot: block.slot,
                    proposer_index: block.proposer_index,
                    parent_root: block.parent_root,
                    state_root: block.state_root,
                    body: bellatrix::BeaconBlockBody {
                        randao_reveal: body.randao_reveal.clone(),
                        eth1_data: body.eth1_data.clone(),
                        graffiti: body.graffiti.clone(),
                        proposer_slashings: body.proposer_slashings.clone(),
                        attester_slashings: body.attester_slashings.clone(),
                        attestations: body.attestations.clone(),
                        deposits: body.deposits.clone(),
                        voluntary_exits: body.voluntary_exits.clone(),
                        sync_aggregate: body.sync_aggregate.clone(),
                        execution_payload: execution_payload.clone(),
                    },
                },
                signature,
            };
            Ok(SignedBeaconBlock::Bellatrix(inner))
        }
        SignedBlindedBeaconBlock::Capella(blinded_block) => {
            let signature = blinded_block.signature.clone();
            let block = &blinded_block.message;
            let body = &block.body;
            let execution_payload = execution_payload.capella().ok_or(Error::InvalidFork {
                expected: Fork::Capella,
                provided: execution_payload.version(),
            })?;

            let inner = capella::SignedBeaconBlock {
                message: capella::BeaconBlock {
                    slot: block.slot,
                    proposer_index: block.proposer_index,
                    parent_root: block.parent_root,
                    state_root: block.state_root,
                    body: capella::BeaconBlockBody {
                        randao_reveal: body.randao_reveal.clone(),
                        eth1_data: body.eth1_data.clone(),
                        graffiti: body.graffiti.clone(),
                        proposer_slashings: body.proposer_slashings.clone(),
                        attester_slashings: body.attester_slashings.clone(),
                        attestations: body.attestations.clone(),
                        deposits: body.deposits.clone(),
                        voluntary_exits: body.voluntary_exits.clone(),
                        sync_aggregate: body.sync_aggregate.clone(),
                        execution_payload: execution_payload.clone(),
                        bls_to_execution_changes: body.bls_to_execution_changes.clone(),
                    },
                },
                signature,
            };
            Ok(SignedBeaconBlock::Capella(inner))
        }
        SignedBlindedBeaconBlock::Deneb(blinded_block) => {
            let signature = blinded_block.signature.clone();
            let block = &blinded_block.message;
            let body = &block.body;
            let execution_payload = execution_payload.deneb().ok_or(Error::InvalidFork {
                expected: Fork::Deneb,
                provided: execution_payload.version(),
            })?;

            let inner = deneb::SignedBeaconBlock {
                message: deneb::BeaconBlock {
                    slot: block.slot,
                    proposer_index: block.proposer_index,
                    parent_root: block.parent_root,
                    state_root: block.state_root,
                    body: deneb::BeaconBlockBody {
                        randao_reveal: body.randao_reveal.clone(),
                        eth1_data: body.eth1_data.clone(),
                        graffiti: body.graffiti.clone(),
                        proposer_slashings: body.proposer_slashings.clone(),
                        attester_slashings: body.attester_slashings.clone(),
                        attestations: body.attestations.clone(),
                        deposits: body.deposits.clone(),
                        voluntary_exits: body.voluntary_exits.clone(),
                        sync_aggregate: body.sync_aggregate.clone(),
                        execution_payload: execution_payload.clone(),
                        bls_to_execution_changes: body.bls_to_execution_changes.clone(),
                        blob_kzg_commitments: body.blob_kzg_commitments.clone(),
                    },
                },
                signature,
            };
            Ok(SignedBeaconBlock::Deneb(inner))
        }
    }
}

fn verify_blinded_block_signature(
    auction_request: &AuctionRequest,
    signed_block: &SignedBlindedBeaconBlock,
    genesis_validators_root: &Root,
    context: &Context,
) -> Result<(), Error> {
    let proposer_public_key = &auction_request.public_key;
    let slot = signed_block.message().slot();
    let domain = compute_consensus_domain(slot, genesis_validators_root, context)?;
    verify_signed_data(
        &signed_block.message(),
        signed_block.signature(),
        proposer_public_key,
        domain,
    )
    .map_err(Into::into)
}

#[derive(Clone)]
pub struct Relay(Arc<Inner>);

impl Deref for Relay {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Inner {
    secret_key: SecretKey,
    public_key: BlsPublicKey,
    validator_registry: ValidatorRegistry,
    proposer_scheduler: ProposerScheduler,
    builder_registry: HashSet<BlsPublicKey>,
    beacon_node: ApiClient,
    context: Context,
    state: Mutex<State>,
    genesis_validators_root: Root,
}

#[derive(Debug, Default)]
struct State {
    // contains validator public keys that have been updated since we last refreshed
    // the proposer scheduler
    outstanding_validator_updates: HashSet<BlsPublicKey>,

    // auction state
    open_auctions: HashSet<AuctionRequest>,
    auctions: HashMap<AuctionRequest, Arc<AuctionContext>>,
    // keeps set of all submissions that are _NOT_ the current best bid.
    // the current best bid is stored in `auctions`.
    other_submissions: HashMap<AuctionRequest, HashSet<AuctionContext>>,
    delivered_payloads: HashMap<AuctionRequest, Arc<AuctionContext>>,
}

impl Relay {
    pub fn new(
        beacon_node: ApiClient,
        secret_key: SecretKey,
        accepted_builders: Vec<BlsPublicKey>,
        context: Context,
        genesis_validators_root: Root,
    ) -> Self {
        let public_key = secret_key.public_key();
        let slots_per_epoch = context.slots_per_epoch;
        let validator_registry = ValidatorRegistry::new(beacon_node.clone(), slots_per_epoch);
        let proposer_scheduler = ProposerScheduler::new(beacon_node.clone(), slots_per_epoch);
        let inner = Inner {
            secret_key,
            public_key,
            validator_registry,
            proposer_scheduler,
            builder_registry: HashSet::from_iter(accepted_builders),
            beacon_node,
            context,
            state: Default::default(),
            genesis_validators_root,
        };
        info!(public_key = %inner.public_key, "relay initialized");
        Self(Arc::new(inner))
    }

    pub async fn on_epoch(&self, epoch: Epoch) {
        info!(epoch, "processing");

        if let Err(err) = self.validator_registry.on_epoch(epoch).await {
            error!(%err, epoch, "could not update validator registry");
        }
        self.refresh_proposer_schedule(epoch).await;

        let retain_slot = epoch.checked_sub(HISTORY_LOOK_BEHIND_EPOCHS).unwrap_or_default() *
            self.context.slots_per_epoch;
        trace!(retain_slot, "pruning stale auctions");
        let mut state = self.state.lock();
        state.auctions.retain(|auction_request, _| auction_request.slot >= retain_slot);
        state.other_submissions.retain(|auction_request, _| auction_request.slot >= retain_slot);
        state.delivered_payloads.retain(|auction_request, _| auction_request.slot >= retain_slot);
    }

    async fn refresh_proposer_schedule(&self, epoch: Epoch) {
        if let Err(err) = self.proposer_scheduler.on_epoch(epoch, &self.validator_registry).await {
            error!(%err, epoch, "could not refresh proposer schedule");
        }
        if let Ok(schedule) = self.proposer_scheduler.get_proposal_schedule() {
            let proposal_slots = schedule
                .into_iter()
                .map(|schedule| (schedule.slot, schedule.validator_index))
                .collect::<Vec<_>>();
            info!(?proposal_slots, "proposer schedule refreshed");
        }
    }

    pub async fn on_slot(&self, slot: Slot) {
        info!(slot, "processing");

        // TODO: no reason to wait for slot boundary,
        // but likely want some more sophisticated channel machinery to dispatch updates
        let keys_to_refresh = {
            let mut state = self.state.lock();
            HashSet::<BlsPublicKey>::from_iter(state.outstanding_validator_updates.drain())
        };
        if !keys_to_refresh.is_empty() {
            // TODO: can be more precise with which proposers to update
            // for now, just refresh them all...
            let epoch = slot / self.context.slots_per_epoch;
            self.refresh_proposer_schedule(epoch).await;
        }

        trace!(retain_slot = slot - AUCTION_LIFETIME_SLOTS, "dropping old auctions");
        let mut state = self.state.lock();
        state
            .open_auctions
            .retain(|auction_request| auction_request.slot + AUCTION_LIFETIME_SLOTS >= slot);
    }

    // TODO: build tip context and support reorgs...
    pub fn on_payload_attributes(&self, event: PayloadAttributesEvent) -> Result<(), Error> {
        trace!(?event, "processing payload attributes");
        let proposer_public_key =
            self.validator_registry.get_public_key(event.proposer_index).ok_or_else::<Error, _>(
                || RelayError::UnknownValidatorIndex(event.proposer_index).into(),
            )?;
        let auction_request = AuctionRequest {
            slot: event.proposal_slot,
            parent_hash: event.parent_block_hash,
            public_key: proposer_public_key,
        };
        let mut state = self.state.lock();
        state.open_auctions.insert(auction_request);
        Ok(())
    }

    fn get_auction_context(&self, auction_request: &AuctionRequest) -> Option<Arc<AuctionContext>> {
        let state = self.state.lock();
        state.auctions.get(auction_request).cloned()
    }

    fn validate_allowed_builder(&self, builder_public_key: &BlsPublicKey) -> Result<(), Error> {
        if self.builder_registry.contains(builder_public_key) {
            Ok(())
        } else {
            Err(RelayError::BuilderNotRegistered(builder_public_key.clone()).into())
        }
    }

    fn validate_auction_request(&self, auction_request: &AuctionRequest) -> Result<(), RelayError> {
        let state = self.state.lock();
        if state.open_auctions.contains(auction_request) {
            Ok(())
        } else {
            let err = RelayError::InvalidAuctionRequest(auction_request.clone());
            Err(err)
        }
    }

    // NOTE: best route is likely through `execution-apis`
    // fn compute_adjusted_gas_limit(&self, preferred_gas_limit: u64) -> u64 {
    //     let parent_gas_limit = unimplemented!("need efficient way to get parent's gas limit");
    //     compute_preferred_gas_limit(preferred_gas_limit, parent_gas_limit)
    // }

    // Assume:
    // - `execution_payload` is valid
    // - pays the proposer the amount claimed in the `bid_trace`
    // - respects the proposer's preferred gas limit, within protocol tolerance
    fn validate_builder_submission_trusted(
        &self,
        bid_trace: &BidTrace,
        execution_payload: &ExecutionPayload,
    ) -> Result<(), RelayError> {
        let proposer_public_key = &bid_trace.proposer_public_key;
        let signed_registration = self
            .validator_registry
            .get_signed_registration(proposer_public_key)
            .ok_or_else(|| RelayError::ValidatorNotRegistered(proposer_public_key.clone()))?;

        if bid_trace.proposer_fee_recipient != signed_registration.message.fee_recipient {
            let fee_recipient = &signed_registration.message.fee_recipient;
            return Err(RelayError::InvalidFeeRecipient(
                proposer_public_key.clone(),
                fee_recipient.clone(),
            ));
        }

        // NOTE: disabled in the "trusted" validation
        // let adjusted_gas_limit =
        //     self.compute_adjusted_gas_limit(signed_registration.message.gas_limit);
        // if bid_trace.gas_limit != adjusted_gas_limit {
        //     return Err(Error::InvalidGasLimitForProposer(
        //         proposer_public_key.clone(),
        //         adjusted_gas_limit,
        //     ))
        // }

        if bid_trace.gas_limit != execution_payload.gas_limit() {
            return Err(RelayError::InvalidGasLimit(
                bid_trace.gas_limit,
                execution_payload.gas_limit(),
            ));
        }

        if bid_trace.gas_used != execution_payload.gas_used() {
            return Err(RelayError::InvalidGasUsed(
                bid_trace.gas_used,
                execution_payload.gas_used(),
            ));
        }

        if &bid_trace.parent_hash != execution_payload.parent_hash() {
            return Err(RelayError::InvalidParentHash(
                bid_trace.parent_hash.clone(),
                execution_payload.parent_hash().clone(),
            ));
        }

        if &bid_trace.block_hash != execution_payload.block_hash() {
            return Err(RelayError::InvalidBlockHash(
                bid_trace.block_hash.clone(),
                execution_payload.block_hash().clone(),
            ));
        }

        Ok(())
    }

    fn insert_bid_if_greater(
        &self,
        auction_request: AuctionRequest,
        signed_submission: &SignedBidSubmission,
        value: U256,
        receive_duration: Duration,
    ) -> Result<(), Error> {
        if let Some(bid) = self.get_auction_context(&auction_request) {
            if bid.value() > value {
                info!(%auction_request, builder_public_key = %bid.builder_public_key(), "block submission was not greater in value; ignoring");
                return Ok(());
            }
        }
        let auction_context = AuctionContext::new(
            signed_submission.clone(),
            receive_duration,
            self.public_key.clone(),
            &self.secret_key,
            &self.context,
        )?;
        let auction_context = Arc::new(auction_context);
        let block_hash = auction_context.execution_payload().block_hash();
        let txn_count = auction_context.execution_payload().transactions().len();
        let blob_count =
            auction_context.blobs_bundle().map(|bundle| bundle.blobs.len()).unwrap_or_default();
        info!(%auction_request, builder_public_key = %auction_context.builder_public_key(), %block_hash, txn_count, blob_count, "inserting new bid");
        let mut state = self.state.lock();
        let old_context = state.auctions.insert(auction_request.clone(), auction_context);

        // NOTE: save other submissions for data APIs
        if let Some(context) = old_context {
            // TODO: better way to remove from `Arc`?
            if let Some(context) = Arc::into_inner(context) {
                let entry = state.other_submissions.entry(auction_request).or_default();
                entry.insert(context);
            }
        }
        Ok(())
    }

    fn store_delivered_payload(
        &self,
        auction_request: AuctionRequest,
        auction_context: Arc<AuctionContext>,
    ) {
        let mut state = self.state.lock();
        if let Some(existing) = state.delivered_payloads.get(&auction_request) {
            if existing != &auction_context {
                error!(
                    ?auction_request,
                    ?auction_context,
                    ?existing,
                    "skipping attempt to store different result for delivered payload"
                );
                return;
            }
        }
        state.delivered_payloads.insert(auction_request, auction_context);
    }
}

#[async_trait]
impl BlindedBlockProvider for Relay {
    async fn register_validators(
        &self,
        registrations: &[SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let current_time = get_current_unix_time_in_nanos().try_into().expect("fits in type");
        let (updated_keys, errs) = self.validator_registry.process_registrations(
            registrations,
            current_time,
            &self.context,
        );

        let updated_key_count = updated_keys.len();
        info!(
            updates = updated_key_count,
            registrations = registrations.len(),
            "processed validator registrations"
        );
        let mut state = self.state.lock();
        state.outstanding_validator_updates.extend(updated_keys);

        if errs.is_empty() {
            Ok(())
        } else {
            warn!(?errs, "error processing some registrations");
            Err(Error::RegistrationErrors(errs))
        }
    }

    async fn fetch_best_bid(
        &self,
        auction_request: &AuctionRequest,
    ) -> Result<SignedBuilderBid, Error> {
        if let Err(err) = self.validate_auction_request(auction_request) {
            warn!(%err, "could not fetch best bid");
            return Err(err.into());
        }

        let auction_context = self
            .get_auction_context(auction_request)
            .ok_or_else(|| Error::NoBidPrepared(auction_request.clone()))?;
        let signed_builder_bid = auction_context.signed_builder_bid();
        info!(%auction_request, %signed_builder_bid, "serving bid");
        Ok(signed_builder_bid.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &SignedBlindedBeaconBlock,
    ) -> Result<AuctionContents, Error> {
        let auction_request = {
            let block = signed_block.message();
            let slot = block.slot();
            let body = block.body();
            let payload_header = body.execution_payload_header();
            let parent_hash = payload_header.parent_hash().clone();
            let proposer_index = block.proposer_index();
            let public_key = self
                .validator_registry
                .get_public_key(proposer_index)
                .ok_or(RelayError::UnknownValidatorIndex(proposer_index))?;
            AuctionRequest { slot, parent_hash, public_key }
        };

        if let Err(err) = self.validate_auction_request(&auction_request) {
            warn!(%err, "could not open bid");
            return Err(err.into());
        }

        let auction_context = self
            .get_auction_context(&auction_request)
            .ok_or_else(|| RelayError::MissingAuction(auction_request.clone()))?;

        {
            let block = signed_block.message();
            let body = block.body();
            let execution_payload_header = body.execution_payload_header();
            let local_header = auction_context.signed_builder_bid().message.header();
            if let Err(err) = validate_header_equality(local_header, execution_payload_header) {
                warn!(%err, %auction_request, "invalid incoming signed blinded beacon block");
                return Err(RelayError::InvalidSignedBlindedBeaconBlock.into());
            }
        }

        if let Err(err) = verify_blinded_block_signature(
            &auction_request,
            signed_block,
            &self.genesis_validators_root,
            &self.context,
        ) {
            warn!(%err, %auction_request, "invalid incoming signed blinded beacon block signature");
            return Err(RelayError::InvalidSignedBlindedBeaconBlock.into());
        }

        match unblind_block(signed_block, auction_context.execution_payload()) {
            Ok(signed_block) => {
                let version = signed_block.version();
                let block_root =
                    signed_block.message().hash_tree_root().map_err(ConsensusError::from)?;
                let request = SubmitSignedBeaconBlock {
                    signed_block: &signed_block,
                    kzg_proofs: auction_context.blobs_bundle().map(|bundle| bundle.proofs.as_ref()),
                    blobs: auction_context.blobs_bundle().map(|bundle| bundle.blobs.as_ref()),
                };
                if let Err(err) = self
                    .beacon_node
                    .post_signed_beacon_block_v2(
                        request,
                        version,
                        Some(BroadcastValidation::ConsensusAndEquivocation),
                    )
                    .await
                {
                    warn!(%err, %auction_request, %block_root, "block failed beacon node validation");
                    Err(RelayError::InvalidSignedBlindedBeaconBlock.into())
                } else {
                    let block_hash = auction_context.execution_payload().block_hash();
                    info!(%auction_request, %block_root, %block_hash, "returning local payload");
                    let auction_contents = auction_context.to_auction_contents();
                    self.store_delivered_payload(auction_request, auction_context);
                    Ok(auction_contents)
                }
            }
            Err(err) => {
                warn!(%err, %auction_request, "invalid incoming signed blinded beacon block");
                return Err(RelayError::InvalidSignedBlindedBeaconBlock.into());
            }
        }
    }
}

#[async_trait]
impl BlindedBlockRelayer for Relay {
    async fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error> {
        let schedule = self.proposer_scheduler.get_proposal_schedule()?;
        let slots = schedule.iter().map(|schedule| schedule.slot).collect::<Vec<_>>();
        debug!(?slots, "sending schedule");
        Ok(schedule)
    }

    async fn submit_bid(&self, signed_submission: &SignedBidSubmission) -> Result<(), Error> {
        let receive_duration = duration_since_unix_epoch();
        let (auction_request, value) = {
            let bid_trace = signed_submission.message();
            let builder_public_key = &bid_trace.builder_public_key;
            self.validate_allowed_builder(builder_public_key)?;

            let auction_request = AuctionRequest {
                slot: bid_trace.slot,
                parent_hash: bid_trace.parent_hash.clone(),
                public_key: bid_trace.proposer_public_key.clone(),
            };
            if let Err(err) = self.validate_auction_request(&auction_request) {
                warn!(%err, "could not validate bid submission");
                return Err(err.into());
            }

            self.validate_builder_submission_trusted(bid_trace, signed_submission.payload())?;
            debug!(%auction_request, "validated builder submission");
            (auction_request, bid_trace.value)
        };

        let message = signed_submission.message();
        let public_key = &signed_submission.message().builder_public_key;
        let signature = signed_submission.signature();
        verify_signed_builder_data(message, public_key, signature, &self.context)?;

        // NOTE: this does _not_ respect cancellations
        // TODO: move to regime where we track best bid by builder
        // and also move logic to cursor best bid for auction off this API
        self.insert_bid_if_greater(auction_request, signed_submission, value, receive_duration)?;

        Ok(())
    }
}

fn payload_trace_from_auction(auction_context: &AuctionContext) -> PayloadTrace {
    let bid_trace = auction_context.bid_trace();
    let builder_bid = &auction_context.signed_builder_bid().message;
    let header = builder_bid.header();
    PayloadTrace {
        slot: bid_trace.slot,
        parent_hash: bid_trace.parent_hash.clone(),
        block_hash: bid_trace.block_hash.clone(),
        builder_public_key: bid_trace.builder_public_key.clone(),
        proposer_public_key: bid_trace.proposer_public_key.clone(),
        proposer_fee_recipient: bid_trace.proposer_fee_recipient.clone(),
        gas_limit: bid_trace.gas_limit,
        gas_used: bid_trace.gas_used,
        value: bid_trace.value,
        block_number: header.block_number(),
        transaction_count: auction_context.execution_payload().transactions().len(),
        blob_count: auction_context
            .blobs_bundle()
            .map(|bundle| bundle.blobs.len())
            .unwrap_or_default(),
    }
}

fn submission_trace_from_auction(auction_context: &AuctionContext) -> SubmissionTrace {
    let bid_trace = auction_context.bid_trace();
    let receive_duration = auction_context.receive_duration();
    let builder_bid = &auction_context.signed_builder_bid().message;
    let header = builder_bid.header();
    SubmissionTrace {
        slot: bid_trace.slot,
        parent_hash: bid_trace.parent_hash.clone(),
        block_hash: bid_trace.block_hash.clone(),
        builder_public_key: bid_trace.builder_public_key.clone(),
        proposer_public_key: bid_trace.proposer_public_key.clone(),
        proposer_fee_recipient: bid_trace.proposer_fee_recipient.clone(),
        gas_limit: bid_trace.gas_limit,
        gas_used: bid_trace.gas_used,
        value: bid_trace.value,
        block_number: header.block_number(),
        transaction_count: auction_context.execution_payload().transactions().len(),
        blob_count: auction_context
            .blobs_bundle()
            .map(|bundle| bundle.blobs.len())
            .unwrap_or_default(),
        timestamp: receive_duration.as_secs(),
        timestamp_ms: receive_duration.as_millis(),
    }
}

#[async_trait]
impl BlindedBlockDataProvider for Relay {
    fn public_key(&self) -> &BlsPublicKey {
        &self.public_key
    }

    fn registered_validators_count(&self) -> usize {
        self.validator_registry.registration_count()
    }

    async fn get_delivered_payloads(
        &self,
        _filters: &DeliveredPayloadFilter,
    ) -> Result<Vec<PayloadTrace>, Error> {
        let state = self.state.lock();
        let mut traces = state
            .delivered_payloads
            .iter()
            .map(|(auction_request, auction_context)| {
                let trace = payload_trace_from_auction(auction_context);
                (auction_request, trace)
            })
            .collect::<Vec<_>>();
        traces.sort_by(|a, b| a.0.cmp(b.0));
        Ok(traces.into_iter().rev().map(|(_, trace)| trace).collect())
    }

    async fn get_block_submissions(
        &self,
        _filters: &BlockSubmissionFilter,
    ) -> Result<Vec<SubmissionTrace>, Error> {
        let state = self.state.lock();
        let mut traces = state
            .auctions
            .iter()
            .map(|(auction_request, auction_context)| {
                let trace = submission_trace_from_auction(auction_context);
                (auction_request.clone(), trace)
            })
            .collect::<Vec<_>>();
        let other_traces = state
            .other_submissions
            .iter()
            .flat_map(|(auction_request, contexts)| {
                contexts.iter().map(|auction_context| {
                    let trace = submission_trace_from_auction(auction_context);
                    (auction_request.clone(), trace)
                })
            })
            .collect::<Vec<_>>();
        traces.extend(other_traces);
        // sort by primarily slot, and then receipt timestamp
        traces.sort_by(|a, b| {
            let auction_request = a.0.cmp(&b.0);
            if let Ordering::Equal = auction_request {
                a.1.timestamp_ms.cmp(&b.1.timestamp_ms)
            } else {
                auction_request
            }
        });
        Ok(traces.into_iter().rev().map(|(_, trace)| trace).collect())
    }

    async fn fetch_validator_registration(
        &self,
        public_key: &BlsPublicKey,
    ) -> Result<SignedValidatorRegistration, Error> {
        self.validator_registry
            .get_signed_registration(public_key)
            .ok_or_else(|| RelayError::ValidatorNotRegistered(public_key.clone()))
            .map_err(Into::into)
    }
}
