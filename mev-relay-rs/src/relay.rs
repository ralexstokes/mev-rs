use async_trait::async_trait;
use beacon_api_client::mainnet::Client;
use ethereum_consensus::{
    builder::ValidatorRegistration,
    clock::get_current_unix_time_in_nanos,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, Root, Slot, U256},
    state_transition::Context,
};
use mev_rs::{
    signing::{compute_consensus_signing_root, sign_builder_message, verify_signature},
    types::{
        BidRequest, BidTrace, BuilderBid, ExecutionPayload, ExecutionPayloadHeader,
        ProposerSchedule, SignedBidSubmission, SignedBlindedBeaconBlock, SignedBuilderBid,
        SignedValidatorRegistration,
    },
    BlindedBlockProvider, BlindedBlockRelayer, Error, ProposerScheduler, ValidatorRegistry,
};
use parking_lot::Mutex;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    sync::Arc,
};
use tracing::error;

// `PROPOSAL_TOLERANCE_DELAY` controls how aggresively the relay drops "old" execution payloads
// once they have been fetched from builders -- currently in response to an incoming request from a
// proposer. Setting this to anything other than `0` incentivizes late proposals and setting it to
// `1` allows for latency at the slot boundary while still enabling a successful proposal.
// TODO likely drop this feature...
const PROPOSAL_TOLERANCE_DELAY: Slot = 1;

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

fn validate_bid_request(_bid_request: &BidRequest) -> Result<(), Error> {
    // TODO validations

    // verify slot is timely

    // verify parent_hash is on a chain tip

    // verify public_key is one of the possible proposers

    Ok(())
}

fn validate_execution_payload(
    execution_payload: &ExecutionPayload,
    _value: &U256,
    preferences: &ValidatorRegistration,
) -> Result<(), Error> {
    // TODO validations

    // TODO allow for "adjustment cap" per the protocol rules
    // towards the proposer's preference
    if execution_payload.gas_limit() != preferences.gas_limit {
        return Err(Error::InvalidGasLimit)
    }

    // verify payload is valid

    // verify payload sends `value` to proposer

    Ok(())
}

fn validate_signed_block(
    signed_block: &mut SignedBlindedBeaconBlock,
    public_key: &BlsPublicKey,
    local_payload: &ExecutionPayload,
    genesis_validators_root: &Root,
    context: &Context,
) -> Result<(), Error> {
    let local_block_hash = local_payload.block_hash();
    let mut block = signed_block.message_mut();

    let body = block.body();
    let payload_header = body.execution_payload_header();
    let block_hash = payload_header.block_hash();
    if block_hash != local_block_hash {
        return Err(Error::InvalidExecutionPayloadInBlock)
    }

    // OPTIONAL:
    // -- verify w/ consensus?
    // verify slot is timely
    // verify proposer_index is correct
    // verify parent_root matches

    let slot = block.slot();
    let signing_root =
        compute_consensus_signing_root(&mut block, slot, genesis_validators_root, context)?;
    let signature = signed_block.signature();
    verify_signature(public_key, signing_root.as_ref(), signature).map_err(Into::into)
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
    genesis_validators_root: Root,
    validator_registry: ValidatorRegistry,
    proposer_scheduler: ProposerScheduler,
    builder_registry: HashSet<BlsPublicKey>,
    context: Context,
    state: Mutex<State>,
}

#[derive(Debug)]
struct BidContext {
    signed_builder_bid: SignedBuilderBid,
    execution_payload: ExecutionPayload,
    value: U256,
}

#[derive(Debug, Default)]
struct State {
    bids: HashMap<BidRequest, BidContext>,
}

impl Relay {
    pub fn new(
        genesis_validators_root: Root,
        beacon_node: Client,
        secret_key: SecretKey,
        accepted_builders: Vec<BlsPublicKey>,
        context: Context,
    ) -> Self {
        let public_key = secret_key.public_key();
        let slots_per_epoch = context.slots_per_epoch;
        let validator_registry = ValidatorRegistry::new(beacon_node.clone(), slots_per_epoch);
        let proposer_scheduler = ProposerScheduler::new(beacon_node, slots_per_epoch);
        let inner = Inner {
            secret_key,
            public_key,
            genesis_validators_root,
            validator_registry,
            proposer_scheduler,
            builder_registry: HashSet::from_iter(accepted_builders),
            context,
            state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    pub async fn on_epoch(&self, epoch: Epoch) {
        if let Err(err) = self.validator_registry.on_epoch(epoch).await {
            error!(%err, "could not load validator set from provided beacon node");
        }
        if let Err(err) = self.proposer_scheduler.on_epoch(epoch, &self.validator_registry).await {
            error!(%err, "could not load proposer duties");
        }
    }

    pub async fn on_slot(&self, slot: Slot) {
        let mut state = self.state.lock();
        state.bids.retain(|bid_request, _| bid_request.slot + PROPOSAL_TOLERANCE_DELAY >= slot);
    }

    fn validate_allowed_builder(&self, builder_public_key: &BlsPublicKey) -> Result<(), Error> {
        if self.builder_registry.contains(builder_public_key) {
            Ok(())
        } else {
            Err(Error::BuilderNotRegistered(builder_public_key.clone()))
        }
    }

    fn validate_bid_request(&self, bid_request: &BidRequest) -> Result<(), Error> {
        validate_bid_request(bid_request)
    }

    fn validate_builder_submission(
        &self,
        _bid_trace: &BidTrace,
        _execution_payload: &ExecutionPayload,
    ) -> Result<(), Error> {
        // TODO:
        // verify payload matches proposer prefs (and proposer is registered)
        // validate_execution_payload(execution_payload, value, preferences)
        // verify bid trace block hash matches execution_payload block hash
        Ok(())
    }

    fn insert_bid_if_greater(
        &self,
        bid_request: BidRequest,
        mut execution_payload: ExecutionPayload,
        value: U256,
    ) -> Result<(), Error> {
        {
            let state = self.state.lock();
            if let Some(bid) = state.bids.get(&bid_request) {
                if bid.value > value {
                    return Ok(())
                }
            }
        }
        let header = to_header(&mut execution_payload)?;
        let mut bid =
            BuilderBid { header, value: value.clone(), public_key: self.public_key.clone() };
        let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;
        let signed_builder_bid = SignedBuilderBid { message: bid, signature };

        let bid_context = BidContext { signed_builder_bid, execution_payload, value };
        let mut state = self.state.lock();
        state.bids.insert(bid_request, bid_context);
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
        self.validator_registry
            .process_registrations(registrations, current_time, &self.context)
            .map_err(Error::RegistrationErrors)
    }

    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error> {
        self.validate_bid_request(bid_request)?;

        let state = self.state.lock();
        let bid_context = state
            .bids
            .get(bid_request)
            .ok_or_else(|| Error::NoBidPrepared(Box::new(bid_request.clone())))?;
        Ok(bid_context.signed_builder_bid.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let block = signed_block.message();
        let slot = block.slot();
        let body = block.body();
        let payload_header = body.execution_payload_header();
        let parent_hash = payload_header.parent_hash().clone();
        let proposer_index = block.proposer_index();
        let public_key = self
            .validator_registry
            .get_public_key(proposer_index)
            .ok_or(Error::ValidatorIndexNotRegistered(proposer_index))?;
        let bid_request = BidRequest { slot, parent_hash, public_key };

        self.validate_bid_request(&bid_request)?;

        let mut state = self.state.lock();
        let bid_context = state
            .bids
            .remove(&bid_request)
            .ok_or_else(|| Error::MissingBid(bid_request.clone()))?;

        let payload = bid_context.execution_payload;
        validate_signed_block(
            signed_block,
            &bid_request.public_key,
            &payload,
            &self.genesis_validators_root,
            &self.context,
        )?;

        // TODO: any other validations required here?

        Ok(payload)
    }
}

#[async_trait]
impl BlindedBlockRelayer for Relay {
    async fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error> {
        self.proposer_scheduler.get_proposal_schedule().map_err(Into::into)
    }

    async fn submit_bid(&self, signed_submission: &mut SignedBidSubmission) -> Result<(), Error> {
        let (bid_request, value) = {
            let bid_trace = &signed_submission.message;
            let builder_public_key = &bid_trace.builder_public_key;
            self.validate_allowed_builder(builder_public_key)?;

            let bid_request = BidRequest {
                slot: bid_trace.slot,
                parent_hash: bid_trace.parent_hash.clone(),
                public_key: bid_trace.proposer_public_key.clone(),
            };
            self.validate_bid_request(&bid_request)?;

            self.validate_builder_submission(bid_trace, &signed_submission.execution_payload)?;
            (bid_request, bid_trace.value.clone())
        };

        signed_submission.verify_signature(&self.context)?;

        let execution_payload = signed_submission.execution_payload.clone();
        // NOTE: this does _not_ respect cancellations
        // TODO: move to regime where we track best bid by builder
        // and also move logic to cursor best bid for auction off this API
        self.insert_bid_if_greater(bid_request, execution_payload, value)?;

        Ok(())
    }
}
