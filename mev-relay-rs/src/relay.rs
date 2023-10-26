use async_trait::async_trait;
use beacon_api_client::{mainnet::Client as ApiClient, BroadcastValidation};
use ethereum_consensus::{
    clock::get_current_unix_time_in_nanos,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Epoch, Root, Slot, U256},
    ssz::prelude::Merkleized,
    state_transition::Context,
    types::mainnet::ExecutionPayloadHeaderRef,
    Error as ConsensusError,
};
use mev_rs::{
    signing::sign_builder_message,
    types::{
        AuctionRequest, BidTrace, BuilderBid, ExecutionPayload, ExecutionPayloadHeader,
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
use tracing::{error, warn};

type PayloadAttributes = ();

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

fn validate_header_equality(
    local_header: &ExecutionPayloadHeader,
    provided_header: ExecutionPayloadHeaderRef<'_>,
) -> Result<(), Error> {
    match local_header {
        ExecutionPayloadHeader::Bellatrix(local_header) => {
            let provided_header =
                provided_header.bellatrix().ok_or(Error::InvalidExecutionPayloadInBlock)?;
            if local_header != provided_header {
                return Err(Error::InvalidExecutionPayloadInBlock);
            }
        }
        ExecutionPayloadHeader::Capella(local_header) => {
            let provided_header =
                provided_header.capella().ok_or(Error::InvalidExecutionPayloadInBlock)?;
            if local_header != provided_header {
                return Err(Error::InvalidExecutionPayloadInBlock);
            }
        }
        ExecutionPayloadHeader::Deneb(local_header) => {
            let provided_header =
                provided_header.deneb().ok_or(Error::InvalidExecutionPayloadInBlock)?;
            if local_header != provided_header {
                return Err(Error::InvalidExecutionPayloadInBlock);
            }
        }
    }
    Ok(())
}

// fn validate_auction_request(_auction_request: &AuctionRequest) -> Result<(), Error> {
//     // TODO validations

//     // verify slot is timely

//     // verify parent_hash is on a chain tip

//     // verify public_key is one of the possible proposers

//     Ok(())
// }

// fn validate_execution_payload(
//     execution_payload: &ExecutionPayload,
//     _value: &U256,
//     preferences: &ValidatorRegistration,
// ) -> Result<(), Error> {
//     // TODO validations

//     // TODO allow for "adjustment cap" per the protocol rules
//     // towards the proposer's preference
//     if execution_payload.gas_limit() != preferences.gas_limit {
//         return Err(Error::InvalidGasLimit)
//     }

//     // verify payload is valid

//     // verify payload sends `value` to proposer

//     Ok(())
// }

// fn validate_signed_block(
//     signed_block: &mut SignedBlindedBeaconBlock,
//     public_key: &BlsPublicKey,
//     local_payload: &ExecutionPayload,
//     genesis_validators_root: &Root,
//     context: &Context,
// ) -> Result<(), Error> {
//     // let local_block_hash = local_payload.block_hash();
//     // let mut block = signed_block.message_mut();

//     // let body = block.body();
//     // let payload_header = body.execution_payload_header();
//     // let block_hash = payload_header.block_hash();
//     // if block_hash != local_block_hash {
//     //     return Err(Error::InvalidExecutionPayloadInBlock)
//     // }

//     // // OPTIONAL:
//     // // -- verify w/ consensus?
//     // // verify slot is timely
//     // // verify proposer_index is correct
//     // // verify parent_root matches

//     // let slot = block.slot();
//     // let signing_root =
//     //     compute_consensus_signing_root(&mut block, slot, genesis_validators_root, context)?;
//     // let signature = signed_block.signature();
//     // verify_signature(public_key, signing_root.as_ref(), signature).map_err(Into::into)
//     Ok(())
// }

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
    beacon_node: ApiClient,
    context: Context,
    state: Mutex<State>,
}

#[derive(Debug)]
struct AuctionContext {
    signed_builder_bid: SignedBuilderBid,
    execution_payload: ExecutionPayload,
    value: U256,
}

#[derive(Debug, Default)]
struct State {
    auctions: HashMap<AuctionRequest, Arc<AuctionContext>>,
}

impl Relay {
    pub fn new(
        genesis_validators_root: Root,
        beacon_node: ApiClient,
        secret_key: SecretKey,
        accepted_builders: Vec<BlsPublicKey>,
        context: Context,
    ) -> Self {
        let public_key = secret_key.public_key();
        let slots_per_epoch = context.slots_per_epoch;
        let validator_registry = ValidatorRegistry::new(beacon_node.clone(), slots_per_epoch);
        let proposer_scheduler = ProposerScheduler::new(beacon_node.clone(), slots_per_epoch);
        let inner = Inner {
            secret_key,
            public_key,
            genesis_validators_root,
            validator_registry,
            proposer_scheduler,
            builder_registry: HashSet::from_iter(accepted_builders),
            beacon_node,
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
        // TODO: sync /w "head slot update"
        state
            .auctions
            .retain(|auction_request, _| auction_request.slot + PROPOSAL_TOLERANCE_DELAY >= slot);
    }

    pub async fn on_payload_attributes(&self, payload_attributes: PayloadAttributes) {
        // TODO
        // store slot as "head slot"
        // store parent block hash
        // get pubkey for index and store as "current bid request"

        // save associated payload attributes
    }

    fn get_auction_context(&self, auction_request: &AuctionRequest) -> Option<Arc<AuctionContext>> {
        let state = self.state.lock();
        state.auctions.get(auction_request).cloned()
    }

    fn validate_allowed_builder(&self, builder_public_key: &BlsPublicKey) -> Result<(), Error> {
        if self.builder_registry.contains(builder_public_key) {
            Ok(())
        } else {
            Err(Error::BuilderNotRegistered(builder_public_key.clone()))
        }
    }

    fn validate_auction_request(&self, auction_request: &AuctionRequest) -> Result<(), Error> {
        // TODO: matches current bid request?
        // validate_auction_request(auction_request)
        Ok(())
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
    ) -> Result<(), Error> {
        let proposer_public_key = &bid_trace.proposer_public_key;
        let signed_registration = self
            .validator_registry
            .get_signed_registration(proposer_public_key)
            .ok_or_else(|| Error::ValidatorNotRegistered(proposer_public_key.clone()))?;

        if bid_trace.proposer_fee_recipient != signed_registration.message.fee_recipient {
            let fee_recipient = &signed_registration.message.fee_recipient;
            return Err(Error::InvalidFeeRecipient(
                proposer_public_key.clone(),
                fee_recipient.clone(),
            ))
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
            return Err(Error::InvalidGasLimit(bid_trace.gas_limit, execution_payload.gas_limit()))
        }

        if bid_trace.gas_used != execution_payload.gas_used() {
            return Err(Error::InvalidGasUsed(bid_trace.gas_used, execution_payload.gas_used()))
        }

        if &bid_trace.parent_hash != execution_payload.parent_hash() {
            return Err(Error::InvalidParentHash(
                bid_trace.parent_hash.clone(),
                execution_payload.parent_hash().clone(),
            ))
        }

        if &bid_trace.block_hash != execution_payload.block_hash() {
            return Err(Error::InvalidBlockHash(
                bid_trace.block_hash.clone(),
                execution_payload.block_hash().clone(),
            ))
        }

        Ok(())
    }

    fn insert_bid_if_greater(
        &self,
        auction_request: AuctionRequest,
        mut execution_payload: ExecutionPayload,
        value: U256,
    ) -> Result<(), Error> {
        if let Some(bid) = self.get_auction_context(&auction_request) {
            if bid.value > value {
                return Ok(())
            }
        }
        let header = to_header(&mut execution_payload)?;
        let mut bid =
            BuilderBid { header, value: value.clone(), public_key: self.public_key.clone() };
        let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;
        let signed_builder_bid = SignedBuilderBid { message: bid, signature };

        let auction_context =
            Arc::new(AuctionContext { signed_builder_bid, execution_payload, value });
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
        self.validator_registry
            .process_registrations(registrations, current_time, &self.context)
            .map_err(Error::RegistrationErrors)
    }

    async fn fetch_best_bid(
        &self,
        auction_request: &AuctionRequest,
    ) -> Result<SignedBuilderBid, Error> {
        self.validate_auction_request(auction_request)?;

        let auction_context = self
            .get_auction_context(auction_request)
            .ok_or_else(|| Error::NoBidPrepared(auction_request.clone()))?;
        Ok(auction_context.signed_builder_bid.clone())
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
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
                .ok_or(Error::ValidatorIndexNotRegistered(proposer_index))?;
            AuctionRequest { slot, parent_hash, public_key }
        };

        self.validate_auction_request(&auction_request)?;

        let auction_context = self
            .get_auction_context(&auction_request)
            .ok_or_else(|| Error::MissingAuction(auction_request.clone()))?;

        {
            let block = signed_block.message();
            let body = block.body();
            let execution_payload_header = body.execution_payload_header();
            let local_header = &auction_context.signed_builder_bid.message.header;
            if let Err(err) = validate_header_equality(local_header, execution_payload_header) {
                warn!(%err, %auction_request, "invalid incoming signed blinded beacon block");
                return Err(Error::InvalidSignedBlindedBeaconBlock)
            }
        }

        if let Err(err) = self
            .beacon_node
            .post_signed_blinded_beacon_block_v2(
                signed_block,
                Some(BroadcastValidation::ConsensusAndEquivocation),
            )
            .await
        {
            let block_root =
                signed_block.message_mut().hash_tree_root().map_err(ConsensusError::from)?;
            warn!(%err, %auction_request, %block_root, "block failed beacon node validation");
            Err(Error::InvalidSignedBlindedBeaconBlock)
        } else {
            let local_payload = &auction_context.execution_payload;
            Ok(local_payload.clone())
        }
    }
}

#[async_trait]
impl BlindedBlockRelayer for Relay {
    async fn get_proposal_schedule(&self) -> Result<Vec<ProposerSchedule>, Error> {
        self.proposer_scheduler.get_proposal_schedule().map_err(Into::into)
    }

    async fn submit_bid(&self, signed_submission: &mut SignedBidSubmission) -> Result<(), Error> {
        let (auction_request, value) = {
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
            (auction_request, bid_trace.value.clone())
        };

        signed_submission.verify_signature(&self.context)?;

        let execution_payload = signed_submission.execution_payload.clone();
        // NOTE: this does _not_ respect cancellations
        // TODO: move to regime where we track best bid by builder
        // and also move logic to cursor best bid for auction off this API
        self.insert_bid_if_greater(auction_request, execution_payload, value)?;

        Ok(())
    }
}
