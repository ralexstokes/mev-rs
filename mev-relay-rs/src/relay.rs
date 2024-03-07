use async_trait::async_trait;
use beacon_api_client::{
    mainnet::Client as ApiClient, BroadcastValidation, PayloadAttributesEvent,
};
use ethereum_consensus::{
    bellatrix::mainnet as bellatrix,
    capella::mainnet as capella,
    clock::get_current_unix_time_in_nanos,
    crypto::SecretKey,
    deneb::mainnet as deneb,
    primitives::{BlsPublicKey, Epoch, Root, Slot, U256},
    ssz::prelude::Merkleized,
    state_transition::Context,
    types::mainnet::{ExecutionPayloadHeaderRef, SignedBeaconBlock},
    Error as ConsensusError, Fork,
};
use mev_rs::{
    signing::{compute_consensus_signing_root, sign_builder_message, verify_signature},
    types::{
        builder_bid, AuctionContents, AuctionRequest, BidTrace, BuilderBid, ExecutionPayload,
        ExecutionPayloadHeader, ProposerSchedule, SignedBidSubmission, SignedBlindedBeaconBlock,
        SignedBuilderBid, SignedValidatorRegistration,
    },
    BlindedBlockProvider, BlindedBlockRelayer, Error, ProposerScheduler, RelayError,
    ValidatorRegistry,
};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};
use tracing::{debug, error, info, trace, warn};

// Sets the lifetime of an auction with respect to its proposal slot.
const AUCTION_LIFETIME_SLOTS: Slot = 1;

fn to_header(execution_payload: &mut ExecutionPayload) -> Result<ExecutionPayloadHeader, Error> {
    let header = match execution_payload {
        ExecutionPayload::Bellatrix(payload) => {
            ExecutionPayloadHeader::Bellatrix(payload.try_into()?)
        }
        ExecutionPayload::Capella(payload) => ExecutionPayloadHeader::Capella(payload.try_into()?),
        ExecutionPayload::Deneb(payload) => ExecutionPayloadHeader::Deneb(payload.try_into()?),
    };
    Ok(header)
}

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

#[derive(Debug)]
struct AuctionContext {
    builder_public_key: BlsPublicKey,
    signed_builder_bid: SignedBuilderBid,
    execution_payload: ExecutionPayload,
    value: U256,
}

#[derive(Debug, Default)]
struct State {
    // contains validator public keys that have been updated since we last refreshed
    // the proposer scheduler
    outstanding_validator_updates: HashSet<BlsPublicKey>,

    // auction state
    open_auctions: HashSet<AuctionRequest>,
    auctions: HashMap<AuctionRequest, Arc<AuctionContext>>,
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
        state
            .auctions
            .retain(|auction_request, _| auction_request.slot + AUCTION_LIFETIME_SLOTS >= slot);
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
            warn!(%err);
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
        mut execution_payload: ExecutionPayload,
        value: U256,
        builder_public_key: BlsPublicKey,
    ) -> Result<(), Error> {
        if let Some(bid) = self.get_auction_context(&auction_request) {
            if bid.value > value {
                info!(%auction_request, %builder_public_key, "block submission was not greater in value; ignoring");
                return Ok(());
            }
        }
        let header = to_header(&mut execution_payload)?;
        let mut bid = match header.version() {
            Fork::Bellatrix => BuilderBid::Bellatrix(builder_bid::bellatrix::BuilderBid {
                header,
                value,
                public_key: self.public_key.clone(),
            }),
            Fork::Capella => BuilderBid::Capella(builder_bid::capella::BuilderBid {
                header,
                value,
                public_key: self.public_key.clone(),
            }),
            Fork::Deneb => unimplemented!(),
            _ => unreachable!("this fork is not reachable from this type"),
        };
        let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;
        let signed_builder_bid = SignedBuilderBid { message: bid, signature };

        let block_hash = execution_payload.block_hash().clone();
        let auction_context = Arc::new(AuctionContext {
            builder_public_key,
            signed_builder_bid,
            execution_payload,
            value,
        });
        info!(%auction_request, builder_public_key = %auction_context.builder_public_key, %block_hash, "inserting new bid");
        let mut state = self.state.lock();
        state.auctions.insert(auction_request, auction_context);
        Ok(())
    }
}

#[async_trait]
impl BlindedBlockProvider for Relay {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
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
        self.validate_auction_request(auction_request)?;

        let auction_context = self
            .get_auction_context(auction_request)
            .ok_or_else(|| Error::NoBidPrepared(auction_request.clone()))?;
        let signed_builder_bid = &auction_context.signed_builder_bid;
        info!(%auction_request, %signed_builder_bid, "serving bid");
        Ok(signed_builder_bid.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
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

        self.validate_auction_request(&auction_request)?;

        let auction_context = self
            .get_auction_context(&auction_request)
            .ok_or_else(|| RelayError::MissingAuction(auction_request.clone()))?;

        {
            let block = signed_block.message();
            let body = block.body();
            let execution_payload_header = body.execution_payload_header();
            let local_header = auction_context.signed_builder_bid.message.header();
            if let Err(err) = validate_header_equality(local_header, execution_payload_header) {
                warn!(%err, %auction_request, "invalid incoming signed blinded beacon block");
                return Err(RelayError::InvalidSignedBlindedBeaconBlock.into())
            }
        }

        verify_blinded_block_signature(&auction_request, signed_block, self)?;

        match unblind_block(signed_block, &auction_context.execution_payload) {
            Ok(mut signed_block) => {
                let version = signed_block.version();
                let block_root =
                    signed_block.message_mut().hash_tree_root().map_err(ConsensusError::from)?;
                if let Err(err) = self
                    .beacon_node
                    .post_signed_beacon_block_v2(
                        &signed_block,
                        version,
                        Some(BroadcastValidation::ConsensusAndEquivocation),
                    )
                    .await
                {
                    warn!(%err, %auction_request, %block_root, "block failed beacon node validation");
                    Err(RelayError::InvalidSignedBlindedBeaconBlock.into())
                } else {
                    let local_payload = &auction_context.execution_payload;
                    let block_hash = local_payload.block_hash();
                    info!(%auction_request, %block_root, %block_hash, "returning local payload");
                    let auction_contents = match local_payload.version() {
                        Fork::Bellatrix => AuctionContents::Bellatrix(local_payload.clone()),
                        Fork::Capella => AuctionContents::Capella(local_payload.clone()),
                        Fork::Deneb => unimplemented!(),
                        _ => unreachable!("fork not reachable from type"),
                    };
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

    async fn submit_bid(&self, signed_submission: &mut SignedBidSubmission) -> Result<(), Error> {
        let (auction_request, value, builder_public_key) = {
            let bid_trace = &signed_submission.message;
            let builder_public_key = &bid_trace.builder_public_key;
            self.validate_allowed_builder(builder_public_key)?;

            let auction_request = AuctionRequest {
                slot: bid_trace.slot,
                parent_hash: bid_trace.parent_hash.clone(),
                public_key: bid_trace.proposer_public_key.clone(),
            };
            self.validate_auction_request(&auction_request)?;

            self.validate_builder_submission_trusted(
                bid_trace,
                &signed_submission.execution_payload,
            )?;
            debug!(%auction_request, "validated builder submission");
            (auction_request, bid_trace.value, bid_trace.builder_public_key.clone())
        };

        signed_submission.verify_signature(&self.context)?;

        let execution_payload = signed_submission.execution_payload.clone();
        // NOTE: this does _not_ respect cancellations
        // TODO: move to regime where we track best bid by builder
        // and also move logic to cursor best bid for auction off this API
        self.insert_bid_if_greater(auction_request, execution_payload, value, builder_public_key)?;

        Ok(())
    }
}

fn verify_blinded_block_signature(
    auction_request: &AuctionRequest,
    signed_block: &mut SignedBlindedBeaconBlock,
    relay: &Relay,
) -> Result<(), Error> {
    let proposer = &auction_request.public_key;
    let slot = signed_block.message().slot();
    let mut block = signed_block.message_mut();
    let signing_root = compute_consensus_signing_root(
        &mut block,
        slot,
        &relay.genesis_validators_root,
        &relay.context,
    )?;

    verify_signature(proposer, signing_root.as_ref(), signed_block.signature()).map_err(Into::into)
}
