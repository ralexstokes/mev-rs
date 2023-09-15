use async_trait::async_trait;
use beacon_api_client::mainnet::Client;
use ethereum_consensus::{
    builder::ValidatorRegistration,
    clock::get_current_unix_time_in_nanos,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Root, Slot, U256},
    state_transition::Context,
};
use mev_build_rs::NullBuilder;
use mev_rs::{
    signing::sign_builder_message,
    types::{
        bellatrix, capella, BidRequest, ExecutionPayload, ExecutionPayloadHeader,
        SignedBlindedBeaconBlock, SignedBuilderBid, SignedValidatorRegistration,
    },
    BlindedBlockProvider, Error, ValidatorRegistry,
};
use parking_lot::Mutex;
use std::{collections::HashMap, ops::Deref, sync::Arc, time::Duration};

// `PROPOSAL_TOLERANCE_DELAY` controls how aggresively the relay drops "old" execution payloads
// once they have been fetched from builders -- currently in response to an incoming request from a
// proposer. Setting this to anything other than `0` incentivizes late proposals and setting it to
// `1` allows for latency at the slot boundary while still enabling a successful proposal.
// TODO likely drop this feature...
const PROPOSAL_TOLERANCE_DELAY: Slot = 1;
const BID_TOLERANCE_DELAY: u128 = 5000;

// TODO: Move this to ethereum_consensus::Clock as a helper method
fn convert_slot_to_timestamp(context: &Context, slot: &Slot) -> u128 {
    let genesis_time = context.genesis_time().expect("Invalid Genesis Time");
    let genesis_time = Duration::from_secs(genesis_time).as_nanos();
    let seconds_per_slot = Duration::from_secs(context.seconds_per_slot).as_nanos();
    u128::from(*slot) * seconds_per_slot + genesis_time
}

fn validate_bid_request(
    is_registered_public_key: &bool,
    current_slot: &Slot,
    bid_request: &BidRequest,
    context: &Context,
) -> Result<(), Error> {
    // TODO validations
    // Convert Slots to timestamps
    let current_slot = convert_slot_to_timestamp(context, current_slot);
    let slot = convert_slot_to_timestamp(context, &bid_request.slot);

    // verify slot is timely
    if slot < current_slot || slot > current_slot + BID_TOLERANCE_DELAY {
        return Err(Error::UntimelyBid)
    }

    // verify parent_hash is on a chain tip
    // TODO: add head event service https://github.com/flashbots/mev-boost-relay/blob/main/beaconclient/prod_beacon_instance.go#L73

    // verify public_key is one of the possible proposers
    if !is_registered_public_key {
        return Err(Error::UnknownBidProposer)
    }

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
    proposer_index: &usize,
    context: &Context,
    current_slot: &Slot,
) -> Result<(), Error> {
    let current_slot = convert_slot_to_timestamp(context, current_slot);
    let slot = convert_slot_to_timestamp(context, &signed_block.slot());

    if signed_block.block_hash() != local_payload.block_hash() {
        return Err(Error::UnknownSignedBlock)
    }

    // OPTIONAL:
    // -- verify w/ consensus?
    // verify slot is timely
    if slot < current_slot || slot > current_slot + BID_TOLERANCE_DELAY {
        return Err(Error::UntimelyBlock)
    }

    // verify proposer_index is correct
    if signed_block.proposer_index() != *proposer_index {
        return Err(Error::UnknownSignedBlock)
    }

    // verify parent_root matches
    if signed_block.parent_hash() != local_payload.parent_hash() {
        return Err(Error::UnknownSignedBlock)
    }

    signed_block.verify_signature(public_key, *genesis_validators_root, context).map_err(From::from)
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
    builder: NullBuilder,
    validator_registry: ValidatorRegistry,
    context: Arc<Context>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    execution_payloads: HashMap<BidRequest, ExecutionPayload>,
    current_slot: Slot,
}

impl Relay {
    pub fn new(
        genesis_validators_root: Root,
        beacon_node: Client,
        secret_key: SecretKey,
        context: Arc<Context>,
    ) -> Self {
        let public_key = secret_key.public_key();
        let validator_registry = ValidatorRegistry::new(beacon_node);
        let inner = Inner {
            secret_key,
            public_key,
            genesis_validators_root,
            builder: NullBuilder,
            validator_registry,
            context,
            state: Default::default(),
        };
        Self(Arc::new(inner))
    }

    async fn load_full_validator_set(&self) {
        if let Err(err) = self.validator_registry.load().await {
            tracing::error!("could not load validator set from provided beacon node; please check config and restart: {err}");
        }
    }

    pub async fn initialize(&self) {
        self.load_full_validator_set().await;
    }

    pub async fn on_slot(&self, slot: Slot, next_epoch: bool) {
        if next_epoch {
            // TODO grab validators more efficiently
            self.load_full_validator_set().await;
        }
        let mut state = self.state.lock();
        state
            .execution_payloads
            .retain(|bid_request, _| bid_request.slot + PROPOSAL_TOLERANCE_DELAY >= slot);
        state.current_slot = slot;
    }
}

#[async_trait]
impl BlindedBlockProvider for Relay {
    async fn register_validators(
        &self,
        registrations: &mut [SignedValidatorRegistration],
    ) -> Result<(), Error> {
        let current_time = get_current_unix_time_in_nanos().try_into().expect("fits in type");
        self.validator_registry.validate_registrations(
            registrations,
            current_time,
            &self.context,
        )?;
        Ok(())
    }

    async fn fetch_best_bid(&self, bid_request: &BidRequest) -> Result<SignedBuilderBid, Error> {
        let is_registered_bid_request =
            self.validator_registry.get_validator_index(&bid_request.public_key).is_some();
        let current_slot = self.state.lock().current_slot;
        validate_bid_request(
            &is_registered_bid_request,
            &current_slot,
            bid_request,
            &self.context,
        )?;

        let public_key = &bid_request.public_key;
        let preferences = self
            .validator_registry
            .get_preferences(public_key)
            .ok_or_else(|| Error::MissingPreferences(public_key.clone()))?;
        let (mut payload, value) = self.builder.get_payload_with_value(
            bid_request,
            &preferences.fee_recipient,
            preferences.gas_limit,
            &self.context,
        )?;

        let header = {
            let mut state = self.state.lock();

            validate_execution_payload(&payload, &value, &preferences)?;

            let header = ExecutionPayloadHeader::try_from(&mut payload)?;

            state.execution_payloads.insert(bid_request.clone(), payload);
            header
        };

        match header {
            ExecutionPayloadHeader::Bellatrix(header) => {
                let mut bid =
                    bellatrix::BuilderBid { header, value, public_key: self.public_key.clone() };
                let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;

                let signed_bid = bellatrix::SignedBuilderBid { message: bid, signature };
                Ok(SignedBuilderBid::Bellatrix(signed_bid))
            }
            ExecutionPayloadHeader::Capella(header) => {
                let mut bid =
                    capella::BuilderBid { header, value, public_key: self.public_key.clone() };
                let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;

                let signed_bid = capella::SignedBuilderBid { message: bid, signature };
                Ok(SignedBuilderBid::Capella(signed_bid))
            }
            ExecutionPayloadHeader::Deneb(_header) => unimplemented!(),
        }
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let slot = signed_block.slot();
        let parent_hash = signed_block.parent_hash().clone();
        let proposer_index = signed_block.proposer_index();
        let public_key =
            self.validator_registry.get_public_key(proposer_index).map_err(Error::from)?;
        let bid_request = BidRequest { slot, parent_hash, public_key };

        let payload = {
            let mut state = self.state.lock();
            state.execution_payloads.remove(&bid_request).ok_or(Error::UnknownBid)?
        };

        let current_slot = self.state.lock().current_slot;
        validate_signed_block(
            signed_block,
            &bid_request.public_key,
            &payload,
            &self.genesis_validators_root,
            &proposer_index,
            &self.context,
            &current_slot,
        )?;

        Ok(payload)
    }
}
