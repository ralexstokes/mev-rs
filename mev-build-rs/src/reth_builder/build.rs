use crate::reth_builder::{
    error::Error,
    reth_compat::{to_bytes32, to_execution_payload, to_u256},
};
use ethereum_consensus::{
    crypto::{hash, SecretKey},
    primitives::{BlsPublicKey, ExecutionAddress, Slot},
    ssz::prelude::*,
    state_transition::Context,
};
use ethers::signers::LocalWallet;
use mev_rs::{
    signing::sign_builder_message,
    types::{BidTrace, SignedBidSubmission},
    Relay,
};
use reth_primitives::{Bytes, ChainSpec, SealedBlock, Withdrawal, B256, U256};
use revm::primitives::{BlockEnv, CfgEnv};
use std::sync::{Arc, Mutex};

pub type BuildIdentifier = ByteVector<4>;

fn make_submission(
    signing_key: &SecretKey,
    builder_public_key: &BlsPublicKey,
    context: &Context,
    build_context: &BuildContext,
    payload: &SealedBlock,
    payment: &U256,
) -> Result<SignedBidSubmission, Error> {
    let mut message = BidTrace {
        slot: build_context.slot,
        parent_hash: to_bytes32(payload.parent_hash),
        block_hash: to_bytes32(payload.hash),
        builder_public_key: builder_public_key.clone(),
        proposer_public_key: build_context.proposer.clone(),
        proposer_fee_recipient: build_context.proposer_fee_recipient.clone(),
        gas_limit: payload.gas_limit,
        gas_used: payload.gas_used,
        value: to_u256(payment),
    };
    let execution_payload = to_execution_payload(payload);
    let signature = sign_builder_message(&mut message, signing_key, context)?;
    Ok(SignedBidSubmission { message, execution_payload, signature })
}

// TODO: drop unnecessary things...
#[derive(Debug, Clone)]
pub struct BuildContext {
    pub slot: Slot,
    pub parent_hash: B256,
    pub proposer: BlsPublicKey,
    pub timestamp: u64,
    pub proposer_fee_recipient: ExecutionAddress,
    pub prev_randao: B256,
    pub withdrawals: Vec<Withdrawal>,
    pub relays: Vec<Arc<Relay>>,
    pub chain_spec: Arc<ChainSpec>,
    pub block_env: BlockEnv,
    pub cfg_env: CfgEnv,
    pub extra_data: Bytes,
    pub builder_wallet: LocalWallet,
    // Amount of gas to reserve after building a payload
    // e.g. used for end-of-block proposer payments
    pub gas_reserve: u64,
    // Amount of the block's value to bid to the proposer
    pub bid_percent: f64,
    // Amount to add to the block's value to bid to the proposer
    pub subsidy: U256,
}

pub fn compute_build_id(slot: Slot, parent_hash: B256, proposer: &BlsPublicKey) -> BuildIdentifier {
    let mut data = Vec::with_capacity(88);
    slot.serialize(&mut data).expect("can serialize");
    parent_hash.serialize(&mut data).expect("can serialize");
    proposer.serialize(&mut data).expect("can serialize");
    let summary = hash(data);
    summary.as_ref()[..4].try_into().unwrap()
}

impl BuildContext {
    pub fn id(&self) -> BuildIdentifier {
        // TODO: cache this
        compute_build_id(self.slot, self.parent_hash, &self.proposer)
    }

    pub fn base_fee(&self) -> u64 {
        self.block_env.basefee.to::<u64>()
    }

    pub fn number(&self) -> u64 {
        self.block_env.number.to::<u64>()
    }

    pub fn gas_limit(&self) -> u64 {
        self.block_env.gas_limit.try_into().unwrap_or(u64::MAX)
    }
}

#[derive(Debug)]
pub struct Build {
    pub context: BuildContext,
    pub state: Mutex<State>,
}

#[derive(Default, Debug)]
pub struct State {
    pub payload_with_payments: PayloadWithPayments,
}

impl Build {
    pub fn new(context: BuildContext) -> Self {
        Self { context, state: Mutex::new(Default::default()) }
    }

    pub fn value(&self) -> U256 {
        let state = self.state.lock().unwrap();
        state.payload_with_payments.proposer_payment
    }

    pub fn prepare_bid(
        &self,
        secret_key: &SecretKey,
        public_key: &BlsPublicKey,
        context: &Context,
    ) -> Result<(SignedBidSubmission, U256), Error> {
        let build_context = &self.context;
        let state = self.state.lock().unwrap();
        let payload_with_payments = &state.payload_with_payments;
        let payload = payload_with_payments
            .payload
            .as_ref()
            .ok_or_else(|| Error::PayloadNotPrepared(build_context.id()))?;
        let payment = &payload_with_payments.proposer_payment;
        let builder_payment = payload_with_payments.builder_payment;
        Ok((
            make_submission(secret_key, public_key, context, build_context, payload, payment)?,
            builder_payment,
        ))
    }
}

#[derive(Debug, Default)]
pub struct PayloadWithPayments {
    pub payload: Option<SealedBlock>,
    pub proposer_payment: U256,
    pub builder_payment: U256,
}
