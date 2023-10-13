use async_trait::async_trait;
use beacon_api_client::mainnet::Client;
use ethereum_consensus::{
    builder::ValidatorRegistration,
    capella::mainnet as capella,
    clock::get_current_unix_time_in_nanos,
    crypto::SecretKey,
    primitives::{BlsPublicKey, Root, Slot, U256},
    state_transition::Context,
};
use mev_rs::{
    signing::{compute_consensus_signing_root, sign_builder_message, verify_signature},
    types::{
        BidRequest, BuilderBid, ExecutionPayload, ExecutionPayloadHeader, SignedBlindedBeaconBlock,
        SignedBuilderBid, SignedValidatorRegistration,
    },
    BlindedBlockProvider, Error, ValidatorRegistry,
};
use parking_lot::Mutex;
use std::{collections::HashMap, ops::Deref, sync::Arc};

// `PROPOSAL_TOLERANCE_DELAY` controls how aggresively the relay drops "old" execution payloads
// once they have been fetched from builders -- currently in response to an incoming request from a
// proposer. Setting this to anything other than `0` incentivizes late proposals and setting it to
// `1` allows for latency at the slot boundary while still enabling a successful proposal.
// TODO likely drop this feature...
const PROPOSAL_TOLERANCE_DELAY: Slot = 1;

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
    if execution_payload.gas_limit() != &preferences.gas_limit {
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
        return Err(Error::UnknownBlock)
    }

    // OPTIONAL:
    // -- verify w/ consensus?
    // verify slot is timely
    // verify proposer_index is correct
    // verify parent_root matches

    let slot = *block.slot();
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
    context: Arc<Context>,
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    execution_payloads: HashMap<BidRequest, ExecutionPayload>,
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
        validate_bid_request(bid_request)?;

        let public_key = &bid_request.public_key;
        let preferences = self
            .validator_registry
            .get_preferences(public_key)
            .ok_or_else(|| Error::MissingPreferences(public_key.clone()))?;

        let value = U256::default();
        let header = {
            let mut payload = ExecutionPayload::Capella(Default::default());
            let mut state = self.state.lock();

            validate_execution_payload(&payload, &value, &preferences)?;

            let inner = payload.capella_mut().unwrap();
            let inner_header = capella::ExecutionPayloadHeader::try_from(inner)?;
            let header = ExecutionPayloadHeader::Capella(inner_header);

            state.execution_payloads.insert(bid_request.clone(), payload);
            header
        };

        let mut bid = BuilderBid { header, value, public_key: self.public_key.clone() };
        let signature = sign_builder_message(&mut bid, &self.secret_key, &self.context)?;
        Ok(SignedBuilderBid { message: bid, signature })
    }

    async fn open_bid(
        &self,
        signed_block: &mut SignedBlindedBeaconBlock,
    ) -> Result<ExecutionPayload, Error> {
        let block = signed_block.message();
        let slot = *block.slot();
        let body = block.body();
        let payload_header = body.execution_payload_header();
        let parent_hash = payload_header.parent_hash().clone();
        let proposer_index = *block.proposer_index();
        let public_key =
            self.validator_registry.get_public_key(proposer_index).map_err(Error::from)?;
        let bid_request = BidRequest { slot, parent_hash, public_key };

        let payload = {
            let mut state = self.state.lock();
            state.execution_payloads.remove(&bid_request).ok_or(Error::UnknownBid)?
        };

        validate_signed_block(
            signed_block,
            &bid_request.public_key,
            &payload,
            &self.genesis_validators_root,
            &self.context,
        )?;

        Ok(payload)
    }
}
