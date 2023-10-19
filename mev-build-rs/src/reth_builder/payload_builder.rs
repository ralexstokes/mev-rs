/// Payload building logic is heavily inspired by
/// the `reth-basic-payload-builder` package in the `reth` codebase.
use crate::reth_builder::{
    build::{BuildContext, PayloadWithPayments},
    cancelled::Cancelled,
    error::Error,
};
use ethers::{
    signers::Signer,
    types::{
        transaction::eip2718::TypedTransaction, Eip1559TransactionRequest, H160,
        U256 as ethers_U256,
    },
};
use reth_interfaces::RethError;
use reth_primitives::{
    constants::{BEACON_NONCE, EMPTY_OMMER_ROOT},
    proofs, Address, Block, Bytes, ChainSpec, Header, IntoRecoveredTransaction, Receipt, Receipts,
    TransactionSigned, TransactionSignedEcRecovered, Withdrawal, B256, U256,
};
use reth_provider::{BundleStateWithReceipts, StateProvider, StateProviderFactory};
use reth_revm::{
    database::StateProviderDatabase, env::tx_env_with_recovered, into_reth_log,
    state_change::post_block_withdrawals_balance_increments,
};
use revm::{
    db::WrapDatabaseRef,
    primitives::{EVMError, Env, InvalidTransaction, ResultAndState},
    Database, DatabaseCommit, State,
};
use std::fmt;

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

fn assemble_txs_from_pool<Pool: reth_transaction_pool::TransactionPool>(
    context: &mut ExecutionContext,
    pool: &Pool,
) -> Result<(), Error> {
    let base_fee = context.build.base_fee();
    let block_gas_limit = context.build.gas_limit();

    let mut best_txs = pool.best_transactions_with_base_fee(base_fee);

    let effective_gas_limit = block_gas_limit - context.build.gas_reserve;
    while let Some(pool_tx) = best_txs.next() {
        if context.is_cancelled() {
            return Ok(())
        }

        // NOTE: we withhold the `gas_reserve` so the "bidder" has some guaranteed room
        // to play with the payload after it is built.
        if context.cumulative_gas_used + pool_tx.gas_limit() > effective_gas_limit {
            best_txs.mark_invalid(&pool_tx);
            continue
        }

        let tx = pool_tx.to_recovered_transaction();

        if let Err(err) = context.extend_transaction(tx) {
            match err {
                Error::Execution(EVMError::Transaction(err)) => {
                    if !matches!(err, InvalidTransaction::NonceTooLow { .. }) {
                        best_txs.mark_invalid(&pool_tx);
                    }
                    continue
                }
                _ => return Err(err),
            }
        }
    }
    Ok(())
}

fn assemble_payload_with_payments(
    mut context: ExecutionContext,
    client: &dyn StateProviderFactory,
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

    let bundle_state = context.bundle_state;
    let transactions_root = proofs::calculate_transaction_root(&context.executed_txs);

    let state_root = client.latest()?.state_root(&bundle_state.clone())?;
    let receipts = bundle_state.receipts_by_block(block_number).to_vec();
    let bundle = BundleStateWithReceipts::new(
        context.db.take_bundle(),
        Receipts::from_vec(vec![receipts]),
        block_number,
    );
    let receipts_root = bundle.receipts_root_slow(block_number).expect("Number is in range");
    let logs_bloom = bundle.block_logs_bloom(block_number).expect("Number is in range");

    let header = Header {
        parent_hash: context.build.parent_hash,
        ommers_hash: EMPTY_OMMER_ROOT,
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

    let payload_with_payments = PayloadWithPayments {
        payload: Some(payload.seal_slow()),
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
    let chain_id = context.build.chain_spec.genesis().config.chain_id;

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
    cancel: &'a Cancelled,
    db: revm::State<WrapDatabaseRef<StateProviderDatabase<Box<dyn StateProvider + 'a>>>>,
    bundle_state: BundleStateWithReceipts,
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

impl<'a> ExecutionContext<'a> {
    fn try_from<P: reth_provider::StateProviderFactory + 'a>(
        context: &'a BuildContext,
        cancel: &'a Cancelled,
        provider: &'a P,
    ) -> Result<Self, Error> {
        let state = provider.state_by_block_hash(context.parent_hash)?;
        let mut db =
            revm::State::builder().with_database_ref(StateProviderDatabase::new(state)).build();
        let bundle_state = BundleStateWithReceipts::new(
            db.take_bundle(),
            Receipts::default(),
            u64::from_le_bytes(context.block_env.number.to_le_bytes()),
        );
        Ok(ExecutionContext {
            build: context,
            cancel,
            db,
            bundle_state,
            cumulative_gas_used: 0,
            total_fees: U256::ZERO,
            executed_txs: Default::default(),
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

        let block_number = self.build.number();
        self.db.commit(state);

        let gas_used = result.gas_used();
        self.cumulative_gas_used += gas_used;
        let receipt = Receipt {
            tx_type: tx.tx_type(),
            success: result.is_success(),
            cumulative_gas_used: self.cumulative_gas_used,
            logs: result.logs().into_iter().map(into_reth_log).collect(),
        };
        self.bundle_state.receipts_by_block(block_number).to_vec().push(Some(receipt));

        let base_fee = self.build.base_fee();
        let fee = tx.effective_tip_per_gas(base_fee).expect("fee is valid; execution succeeded");
        self.total_fees += U256::from(fee) * U256::from(gas_used);

        self.executed_txs.push(tx.into_signed());

        Ok(())
    }
}

pub fn build_payload<
    Provider: reth_provider::StateProviderFactory,
    Pool: reth_transaction_pool::TransactionPool,
>(
    context: &BuildContext,
    threshold_value: U256,
    provider: &Provider,
    pool: &Pool,
    cancel: &Cancelled,
) -> Result<BuildOutcome, Error> {
    let mut context = ExecutionContext::try_from(context, cancel, provider)?;

    if context.is_cancelled() {
        return Ok(BuildOutcome::Cancelled)
    }
    assemble_txs_from_pool(&mut context, pool)?;

    if context.total_fees < threshold_value {
        return Ok(BuildOutcome::Worse { threshold: threshold_value, provided: context.total_fees })
    }

    context.compute_payment_from_fees();

    let payment_tx = construct_payment_tx(&mut context)?;

    if context.is_cancelled() {
        return Ok(BuildOutcome::Cancelled)
    }

    context.extend_transaction(payment_tx)?;

    if context.is_cancelled() {
        return Ok(BuildOutcome::Cancelled)
    }

    assemble_payload_with_payments(context, provider)
}
