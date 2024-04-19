/// Payload building logic is heavily inspired by
/// the `reth-basic-payload-builder` package in the `reth` codebase.
use crate::reth_builder::{
    build::{BuildContext, PayloadWithPayments},
    error::Error,
};
use ethers::{
    signers::Signer,
    types::{
        transaction::eip2718::TypedTransaction, Eip1559TransactionRequest, H160,
        U256 as ethers_U256,
    },
};
use reth_basic_payload_builder::{
    default_payload_builder, BuildArguments, BuildOutcome as RethOutcome, Cancelled, PayloadConfig,
};
use reth_interfaces::RethError;
use reth_payload_builder::{error::PayloadBuilderError, BuiltPayload, PayloadId};
use reth_primitives::{
    constants::{BEACON_NONCE, EMPTY_OMMER_ROOT_HASH},
    proofs,
    revm::{compat::into_reth_log, env::tx_env_with_recovered},
    Address, Block, Bytes, ChainSpec, Header, Receipt, Receipts, TransactionSigned,
    TransactionSignedEcRecovered, Withdrawal, B256, U256,
};
use reth_provider::{BundleStateWithReceipts, StateProvider, StateProviderFactory};
use reth_revm::{
    database::StateProviderDatabase, state_change::post_block_withdrawals_balance_increments,
};
use revm::{
    db::{states::bundle_state::BundleRetention, WrapDatabaseRef},
    primitives::{Env, ResultAndState},
    Database, DatabaseCommit, State,
};
use std::{fmt, sync::Arc};

pub struct RethPayloadBuilder<Pool, Client> {
    build_arguments: BuildArguments<Pool, Client>,
}

impl<Pool, Client> RethPayloadBuilder<Pool, Client>
where
    Client: reth_provider::StateProviderFactory,
    Pool: reth_transaction_pool::TransactionPool,
{
    pub fn new(
        context: &BuildContext,
        client: Client,
        pool: Pool,
        cancel: Cancelled,
        best_payload: Option<Arc<BuiltPayload>>,
    ) -> Self {
        let cached_reads = Default::default();
        let config = PayloadConfig::new(
            context.parent_block.clone(),
            context.extra_data.clone(),
            context.payload_attributes.clone(),
            context.chain_spec.clone(),
        );

        let build_arguments =
            BuildArguments::new(client, pool, cached_reads, config, cancel, best_payload);

        Self { build_arguments }
    }

    pub fn build(self) -> Result<RethOutcome, PayloadBuilderError> {
        default_payload_builder(self.build_arguments)
    }
}

fn process_withdrawals<DB: Database<Error = RethError>>(
    withdrawals: &[Withdrawal],
    chain_spec: &ChainSpec,
    db: &mut State<DB>,
    timestamp: u64,
) -> Result<B256, Error> {
    let balance_increments =
        post_block_withdrawals_balance_increments(chain_spec, timestamp, withdrawals);
    db.increment_balances(balance_increments)?;
    let withdrawals_root = proofs::calculate_withdrawals_root(withdrawals);
    Ok(withdrawals_root)
}

pub enum BuildOutcome {
    BetterOrEqual(PayloadWithPayments),
    // The `provided` value that did not exceed the `threshold` value requested
    Worse { threshold: U256, provided: U256 },
    Cancelled,
}

fn assemble_payload_with_payments<P: StateProviderFactory>(
    mut context: ExecutionContext,
    client: P,
) -> Result<BuildOutcome, Error> {
    let base_fee = context.build.base_fee();
    let block_number = context.build.number();
    let block_gas_limit = context.build.gas_limit();

    let withdrawals_root = process_withdrawals(
        &context.build.withdrawals,
        &context.build.chain_spec,
        &mut context.db,
        context.build.timestamp,
    )?;

    if context.is_cancelled() {
        return Ok(BuildOutcome::Cancelled)
    }

    context.db.merge_transitions(BundleRetention::PlainState);

    let bundle = BundleStateWithReceipts::new(
        context.db.take_bundle(),
        Receipts::from_vec(vec![context.receipts]),
        block_number,
    );
    let receipts_root = bundle.receipts_root_slow(block_number).expect("number is in range");
    let logs_bloom = bundle.block_logs_bloom(block_number).expect("number is in range");
    let state_root = client.latest()?.state_root(&bundle)?;

    let transactions_root = proofs::calculate_transaction_root(&context.executed_txs);

    let header = Header {
        parent_hash: context.build.parent_hash,
        ommers_hash: EMPTY_OMMER_ROOT_HASH,
        beneficiary: context.build.block_env.coinbase,
        state_root,
        transactions_root,
        withdrawals_root: Some(withdrawals_root),
        receipts_root,
        logs_bloom,
        timestamp: context.build.timestamp,
        mix_hash: B256::from_slice(context.build.prev_randao.as_ref()),
        nonce: BEACON_NONCE,
        base_fee_per_gas: Some(base_fee),
        number: block_number,
        gas_limit: block_gas_limit,
        difficulty: U256::ZERO,
        gas_used: context.cumulative_gas_used,
        extra_data: context.build.extra_data.clone(),
        blob_gas_used: None,
        excess_blob_gas: None,
        parent_beacon_block_root: None,
    };

    let payload = Block {
        header,
        body: context.executed_txs,
        ommers: vec![],
        withdrawals: Some(context.build.withdrawals.clone()),
    };

    let payload = BuiltPayload::new(
        PayloadId::new(Default::default()),
        payload.seal_slow(),
        context.total_fees,
    );

    let payload_with_payments = PayloadWithPayments {
        payload: Some(Arc::new(payload)),
        proposer_payment: context.total_payment,
        builder_payment: context.revenue,
    };
    Ok(BuildOutcome::BetterOrEqual(payload_with_payments))
}

fn construct_payment_tx(
    context: &mut ExecutionContext,
) -> Result<TransactionSignedEcRecovered, Error> {
    let sender = context.build.builder_wallet.address();
    let reth_sender = Address::from(sender.to_fixed_bytes());
    let signer_account = context.db.load_cache_account(reth_sender)?;
    let nonce = signer_account.account_info().expect("account exists").nonce;
    let chain_id = context.build.chain_spec.chain().id();

    let fee_recipient = H160::from_slice(context.build.proposer_fee_recipient.as_ref());
    let value = ethers_U256::from_big_endian(&context.total_payment.to_be_bytes::<32>());
    let tx = Eip1559TransactionRequest::new()
        .from(sender)
        .to(fee_recipient)
        // TODO: support smart contract payments
        .gas(21000)
        .max_fee_per_gas(context.build.base_fee())
        .max_priority_fee_per_gas(0)
        .value(value)
        .data(ethers::types::Bytes::default())
        .access_list(ethers::types::transaction::eip2930::AccessList::default())
        .nonce(nonce)
        .chain_id(chain_id);

    let tx = TypedTransaction::Eip1559(tx);
    let wallet = &context.build.builder_wallet;
    let signature = wallet.sign_transaction_sync(&tx).expect("can make transaction");
    let tx_encoded = tx.rlp_signed(&signature);

    let tx_encoded = Bytes::from(tx_encoded.0);
    let payment_tx = TransactionSigned::decode_enveloped(tx_encoded).expect("can decode valid txn");

    Ok(TransactionSignedEcRecovered::from_signed_transaction(payment_tx, reth_sender))
}

struct ExecutionContext<'a> {
    build: &'a BuildContext,
    cancel: Cancelled,
    db: revm::State<WrapDatabaseRef<StateProviderDatabase<Box<dyn StateProvider + 'a>>>>,
    receipts: Vec<Option<Receipt>>,
    cumulative_gas_used: u64,
    total_fees: U256,
    executed_txs: Vec<TransactionSigned>,
    total_payment: U256,
    revenue: U256,
}

impl<'a> fmt::Debug for ExecutionContext<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionContext")
            .field("build", &self.build)
            .field("cumulative_gas_used", &self.cumulative_gas_used)
            .field("total_fees", &self.total_fees)
            .field("total_payment", &self.total_payment)
            .field("revenue", &self.revenue)
            .finish()
    }
}

type DB<'a> = revm::State<WrapDatabaseRef<StateProviderDatabase<Box<dyn StateProvider + 'a>>>>;

impl<'a> ExecutionContext<'a> {
    fn try_from(
        context: &'a BuildContext,
        cancel: Cancelled,
        db: DB<'a>,
        payload: BuiltPayload,
    ) -> Result<Self, Error> {
        Ok(ExecutionContext {
            build: context,
            cancel,
            db,
            receipts: Default::default(),
            cumulative_gas_used: 0,
            total_fees: payload.fees(),
            executed_txs: payload.block().body.clone(),
            total_payment: U256::ZERO,
            revenue: U256::ZERO,
        })
    }

    fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    fn compute_payment_from_fees(&mut self) {
        let integral_percent = (self.build.bid_percent * 100.0) as u64;
        let payment = self.total_fees * U256::from(integral_percent) / U256::from(100);
        self.revenue = self.total_fees - payment;
        self.total_payment = self.build.subsidy + payment;
    }

    fn extend_transaction(&mut self, tx: TransactionSignedEcRecovered) -> Result<(), Error> {
        let env = Env {
            cfg: self.build.cfg_env.clone(),
            block: self.build.block_env.clone(),
            tx: tx_env_with_recovered(&tx),
        };

        let mut evm = revm::EVM::with_env(env);
        evm.database(&mut self.db);

        let ResultAndState { result, state } = evm.transact()?;

        self.db.commit(state);

        let gas_used = result.gas_used();
        self.cumulative_gas_used += gas_used;

        let receipt = Receipt {
            tx_type: tx.tx_type(),
            success: result.is_success(),
            cumulative_gas_used: self.cumulative_gas_used,
            logs: result.logs().into_iter().map(into_reth_log).collect(),
        };
        self.receipts.push(Some(receipt));

        let base_fee = self.build.base_fee();
        let fee =
            tx.effective_tip_per_gas(Some(base_fee)).expect("fee is valid; execution succeeded");
        self.total_fees += U256::from(fee) * U256::from(gas_used);

        self.executed_txs.push(tx.into_signed());

        Ok(())
    }
}

pub fn build_payload<
    Provider: reth_provider::StateProviderFactory + Clone,
    Pool: reth_transaction_pool::TransactionPool,
>(
    context: &BuildContext,
    best_payload: Option<Arc<BuiltPayload>>,
    client: Provider,
    pool: Pool,
    cancel: Cancelled,
) -> Result<BuildOutcome, Error> {
    let payload_builder = RethPayloadBuilder::new(
        context,
        client.clone(),
        pool,
        cancel.clone(),
        best_payload.clone(),
    );
    match payload_builder.build() {
        Ok(RethOutcome::Aborted { fees, .. }) => Ok(BuildOutcome::Worse {
            threshold: best_payload.map(|p| p.fees()).unwrap_or_default(),
            provided: fees,
        }),
        // TODO: leverage cached reads
        Ok(RethOutcome::Better { payload, .. }) => {
            let client_handle = client.clone();
            let state_provider = client_handle.state_by_block_hash(context.parent_hash)?;
            let state = StateProviderDatabase::new(state_provider);
            let db = State::builder().with_database_ref(state).with_bundle_update().build();
            let mut context = ExecutionContext::try_from(context, cancel, db, payload)?;

            context.compute_payment_from_fees();

            let payment_tx = construct_payment_tx(&mut context)?;

            if context.is_cancelled() {
                return Ok(BuildOutcome::Cancelled)
            }

            // NOTE: assume payment transaction always succeeds
            context.extend_transaction(payment_tx)?;

            if context.is_cancelled() {
                return Ok(BuildOutcome::Cancelled)
            }

            assemble_payload_with_payments(context, client)
        }
        Ok(RethOutcome::Cancelled) => Ok(BuildOutcome::Cancelled),
        Err(err) => Err(err.into()),
    }
}
