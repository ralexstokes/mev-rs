use crate::payload::{attributes::BuilderPayloadBuilderAttributes, job::PayloadFinalizerConfig};
use alloy_signer::SignerSync;
use alloy_signer_wallet::LocalWallet;
use reth::{
    api::{ConfigureEvm, PayloadBuilderAttributes},
    payload::{error::PayloadBuilderError, EthBuiltPayload, PayloadId},
    primitives::{
        constants::{
            eip4844::MAX_DATA_GAS_PER_BLOCK, BEACON_NONCE, EMPTY_RECEIPTS, EMPTY_TRANSACTIONS,
        },
        eip4844::calculate_excess_blob_gas,
        proofs::{self, calculate_requests_root},
        revm::env::tx_env_with_recovered,
        Address, Block, ChainId, Header, IntoRecoveredTransaction, Receipt, Receipts, SealedBlock,
        Signature, Transaction, TransactionSigned, TransactionSignedEcRecovered, TxEip1559, TxKind,
        EMPTY_OMMER_ROOT_HASH, U256,
    },
    providers::{BundleStateWithReceipts, StateProviderFactory, StateRootProvider},
    revm::{
        database::StateProviderDatabase,
        db::states::bundle_state::BundleRetention,
        primitives::{EVMError, EnvWithHandlerCfg, InvalidTransaction, ResultAndState},
        state_change::apply_blockhashes_update,
        DatabaseCommit, State,
    },
    transaction_pool::{BestTransactionsAttributes, TransactionPool},
};
use reth_basic_payload_builder::{
    commit_withdrawals, is_better_payload, post_block_withdrawal_requests_contract_call,
    pre_block_beacon_root_contract_call, BuildArguments, BuildOutcome, PayloadConfig,
    WithdrawalsOutcome,
};
use reth_evm_ethereum::{eip6110::parse_deposits_from_receipts, EthEvmConfig};
use reth_interfaces::RethError;
use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, Mutex},
};
use thiserror::Error;
use tokio::sync::mpsc::Sender;
use tracing::{debug, trace, warn};

#[derive(Debug, Error)]
pub enum Error {
    #[error("block gas used {gas_used} exceeded block gas limit {gas_limit}")]
    BlockGasLimitExceeded { gas_used: u64, gas_limit: u64 },
}

pub const BASE_TX_GAS_LIMIT: u64 = 21000;

pub const PAYMENT_TO_CONTRACT_GAS_LIMIT: u64 = 100_000;

fn make_payment_transaction(
    signer: &LocalWallet,
    config: &PayloadFinalizerConfig,
    chain_id: ChainId,
    nonce: u64,
    gas_limit: u64,
    max_fee_per_gas: u128,
    value: U256,
) -> Result<TransactionSignedEcRecovered, PayloadBuilderError> {
    let tx = Transaction::Eip1559(TxEip1559 {
        chain_id,
        nonce,
        gas_limit,
        max_fee_per_gas,
        max_priority_fee_per_gas: 0,
        to: TxKind::Call(config.proposer_fee_recipient),
        value,
        access_list: Default::default(),
        input: Default::default(),
    });
    let signature_hash = tx.signature_hash();
    let signature = signer.sign_hash_sync(&signature_hash).expect("can sign");
    let signed_transaction = TransactionSigned::from_transaction_and_signature(
        tx,
        Signature { r: signature.r(), s: signature.s(), odd_y_parity: signature.v().y_parity() },
    );
    Ok(TransactionSignedEcRecovered::from_signed_transaction(signed_transaction, signer.address()))
}

fn append_payment<Client: StateProviderFactory>(
    client: Client,
    bundle_state_with_receipts: BundleStateWithReceipts,
    signer: &LocalWallet,
    config: &PayloadFinalizerConfig,
    chain_id: ChainId,
    block: SealedBlock,
    value: U256,
    evm_config: EthEvmConfig,
) -> Result<SealedBlock, PayloadBuilderError> {
    let state_provider = client.state_by_block_hash(block.header.header().parent_hash)?;
    let state = StateProviderDatabase::new(&state_provider);
    // TODO: use cached reads
    let mut db = State::builder()
        .with_database_ref(state)
        // TODO skip clone here...
        .with_bundle_prestate(bundle_state_with_receipts.state().clone())
        .with_bundle_update()
        .build();

    let signer_account = db.load_cache_account(signer.address())?;
    let nonce = signer_account.account_info().map(|account| account.nonce).unwrap_or_default();

    let proposer_fee_recipient_account = db.load_cache_account(config.proposer_fee_recipient)?;
    let is_empty_code_hash = proposer_fee_recipient_account
        .account_info()
        .map(|account| account.is_empty_code_hash())
        .unwrap_or_default();

    // Use a fixed gas limit for the payment transaction reflecting the recipient's status
    // as smart contract or EOA.
    let gas_limit =
        if is_empty_code_hash { BASE_TX_GAS_LIMIT } else { PAYMENT_TO_CONTRACT_GAS_LIMIT };

    // SAFETY: cast to bigger type always succeeds
    let max_fee_per_gas = block.header().base_fee_per_gas.unwrap_or_default() as u128;
    let payment_tx = make_payment_transaction(
        signer,
        config,
        chain_id,
        nonce,
        gas_limit,
        max_fee_per_gas,
        value,
    )?;

    // TODO: skip clones here
    let mut env: EnvWithHandlerCfg = EnvWithHandlerCfg::new_with_cfg_env(
        config.cfg_env.clone(),
        config.block_env.clone(),
        tx_env_with_recovered(&payment_tx),
    );
    // NOTE: adjust gas limit to allow for payment transaction
    env.block.gas_limit += U256::from(BASE_TX_GAS_LIMIT);
    let mut evm = evm_config.evm_with_env(&mut db, env);

    let ResultAndState { result, state } =
        evm.transact().map_err(PayloadBuilderError::EvmExecutionError)?;

    drop(evm);
    db.commit(state);

    let Block { mut header, mut body, ommers, withdrawals, requests } = block.unseal();

    // Verify we reserved the correct amount of gas for the payment transaction
    let gas_limit = header.gas_limit + result.gas_used();
    let cumulative_gas_used = header.gas_used + result.gas_used();
    if cumulative_gas_used > gas_limit {
        return Err(PayloadBuilderError::Other(Box::new(Error::BlockGasLimitExceeded {
            gas_used: cumulative_gas_used,
            gas_limit: header.gas_limit,
        })))
    }
    let receipt = Receipt {
        tx_type: payment_tx.tx_type(),
        success: result.is_success(),
        cumulative_gas_used,
        logs: result.into_logs().into_iter().map(Into::into).collect(),
    };

    body.push(payment_tx.into_signed());

    db.merge_transitions(BundleRetention::PlainState);

    let block_number = header.number;
    // TODO skip clone here
    let mut receipts = bundle_state_with_receipts.receipts_by_block(block_number).to_vec();
    receipts.push(Some(receipt));

    let receipts = Receipts::from_vec(vec![receipts]);

    let bundle = BundleStateWithReceipts::new(db.take_bundle(), receipts, block_number);

    let receipts_root = bundle.receipts_root_slow(block_number).expect("Number is in range");
    let logs_bloom = bundle.block_logs_bloom(block_number).expect("Number is in range");
    let state_root = {
        let state_provider = db.database.0;
        state_provider.0.state_root(bundle.state())?
    };
    let transactions_root = proofs::calculate_transaction_root(&body);

    header.state_root = state_root;
    header.transactions_root = transactions_root;
    header.receipts_root = receipts_root;
    header.logs_bloom = logs_bloom;
    header.gas_used = cumulative_gas_used;
    header.gas_limit = gas_limit;

    let block = Block { header, body, ommers, withdrawals, requests };

    Ok(block.seal_slow())
}

#[derive(Debug, Clone)]
pub struct PayloadBuilder(Arc<Inner>);

impl Deref for PayloadBuilder {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct Inner {
    bids: Sender<EthBuiltPayload>,
    signer: LocalWallet,
    pub fee_recipient: Address,
    chain_id: ChainId,
    evm_config: EthEvmConfig,
    states: Mutex<HashMap<PayloadId, BundleStateWithReceipts>>,
}

impl PayloadBuilder {
    pub fn new(
        bids: Sender<EthBuiltPayload>,
        signer: LocalWallet,
        fee_recipient: Address,
        chain_id: ChainId,
    ) -> Self {
        let evm_config = EthEvmConfig::default();
        let inner =
            Inner { bids, signer, fee_recipient, chain_id, evm_config, states: Default::default() };
        Self(Arc::new(inner))
    }

    pub fn get_build_state(&self, payload_id: PayloadId) -> Option<BundleStateWithReceipts> {
        let mut state = self.states.lock().expect("can lock");
        state.remove(&payload_id)
    }

    pub async fn finalize_payload_and_dispatch<Client: StateProviderFactory>(
        &self,
        client: Client,
        payload: EthBuiltPayload,
        payment_amount: U256,
        config: &PayloadFinalizerConfig,
    ) {
        let blob_sidecars = payload.sidecars().to_vec();
        match self.finalize_payload(
            payload.id(),
            client,
            payload.block().clone(),
            payment_amount,
            config,
        ) {
            Ok(mut payload) => {
                payload.extend_sidecars(blob_sidecars);
                if let Err(err) = self.bids.send(payload).await {
                    let payload = err.0;
                    warn!(?payload, "could not send payload to auctioneer");
                }
            }
            Err(err) => {
                warn!(?err, "builder could not finalize payload for auction");
            }
        }
    }

    pub fn finalize_payload<Client: StateProviderFactory>(
        &self,
        payload_id: PayloadId,
        client: Client,
        block: SealedBlock,
        payment_amount: U256,
        config: &PayloadFinalizerConfig,
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        let bundle_state_with_receipts = self
            .get_build_state(payload_id)
            .ok_or_else(|| PayloadBuilderError::Other("missing build state for payload".into()))?;
        let block = append_payment(
            client,
            bundle_state_with_receipts,
            &self.signer,
            config,
            self.chain_id,
            block,
            payment_amount,
            self.evm_config.clone(),
        )?;
        Ok(EthBuiltPayload::new(payload_id, block, payment_amount))
    }
}

impl<Pool, Client> reth_basic_payload_builder::PayloadBuilder<Pool, Client> for PayloadBuilder
where
    Client: StateProviderFactory,
    Pool: TransactionPool,
{
    type Attributes = BuilderPayloadBuilderAttributes;
    type BuiltPayload = EthBuiltPayload;

    fn try_build(
        &self,
        args: BuildArguments<Pool, Client, Self::Attributes, Self::BuiltPayload>,
    ) -> Result<BuildOutcome<Self::BuiltPayload>, PayloadBuilderError> {
        let payload_id = args.config.payload_id();
        let (outcome, bundle) = default_ethereum_payload_builder(self.evm_config.clone(), args)?;
        if let Some(bundle) = bundle {
            let mut states = self.states.lock().expect("can lock");
            states.insert(payload_id, bundle);
        }
        Ok(outcome)
    }

    fn build_empty_payload(
        client: &Client,
        config: PayloadConfig<Self::Attributes>,
    ) -> Result<Self::BuiltPayload, PayloadBuilderError> {
        // TODO: this should also store bundle state for finalization -- do we need to keep it
        // separate from the main driver?
        let extra_data = config.extra_data();
        let PayloadConfig {
            initialized_block_env,
            parent_block,
            attributes,
            chain_spec,
            initialized_cfg,
            ..
        } = config;

        debug!(target: "payload_builder", parent_hash = ?parent_block.hash(), parent_number = parent_block.number, "building empty payload");

        let state = client.state_by_block_hash(parent_block.hash()).map_err(|err| {
                warn!(target: "payload_builder", parent_hash=%parent_block.hash(), %err, "failed to get state for empty payload");
                err
            })?;
        let mut db = State::builder()
            .with_database(StateProviderDatabase::new(state))
            .with_bundle_update()
            .build();

        let base_fee = initialized_block_env.basefee.to::<u64>();
        let block_number = initialized_block_env.number.to::<u64>();
        let block_gas_limit: u64 = initialized_block_env.gas_limit.try_into().unwrap_or(u64::MAX);

        // apply eip-4788 pre block contract call
        pre_block_beacon_root_contract_call(
                &mut db,
                &chain_spec,
                block_number,
                &initialized_cfg,
                &initialized_block_env,
                &attributes,
            ).map_err(|err| {
                warn!(target: "payload_builder", parent_hash=%parent_block.hash(), %err, "failed to apply beacon root contract call for empty payload");
                err
            })?;

        // apply eip-2935 blockhashes update
        apply_blockhashes_update(
            &chain_spec,
            initialized_block_env.timestamp.to::<u64>(),
            block_number,
            &mut db,
        ).map_err(|err| {
            warn!(target: "payload_builder", parent_hash=%parent_block.hash(), %err, "failed to update blockhashes for empty payload");
            PayloadBuilderError::Internal(err.into())
        })?;

        let WithdrawalsOutcome { withdrawals_root, withdrawals } =
                commit_withdrawals(&mut db, &chain_spec, attributes.timestamp(), attributes.withdrawals().clone()).map_err(|err| {
                    warn!(target: "payload_builder", parent_hash=%parent_block.hash(), %err, "failed to commit withdrawals for empty payload");
                    err
                })?;

        // Calculate the requests and the requests root.
        let (requests, requests_root) =
            if chain_spec.is_prague_active_at_timestamp(attributes.timestamp()) {
                // We do not calculate the EIP-6110 deposit requests because there are no
                // transactions in an empty payload.
                let withdrawal_requests = post_block_withdrawal_requests_contract_call(
                    &mut db,
                    &chain_spec,
                    &initialized_cfg,
                    &initialized_block_env,
                    &attributes,
                )?;

                let requests = withdrawal_requests;
                let requests_root = calculate_requests_root(&requests);
                (Some(requests.into()), Some(requests_root))
            } else {
                (None, None)
            };

        // merge all transitions into bundle state, this would apply the withdrawal balance
        // changes and 4788 contract call
        db.merge_transitions(BundleRetention::PlainState);

        // calculate the state root
        let bundle_state = db.take_bundle();
        let state_root = db.database.state_root(&bundle_state).map_err(|err| {
            warn!(target: "payload_builder",
                parent_hash=%parent_block.hash(),
                %err,
                "failed to calculate state root for empty payload"
            );
            err
        })?;

        let mut excess_blob_gas = None;
        let mut blob_gas_used = None;

        if chain_spec.is_cancun_active_at_timestamp(attributes.timestamp()) {
            excess_blob_gas = if chain_spec.is_cancun_active_at_timestamp(parent_block.timestamp) {
                let parent_excess_blob_gas = parent_block.excess_blob_gas.unwrap_or_default();
                let parent_blob_gas_used = parent_block.blob_gas_used.unwrap_or_default();
                Some(calculate_excess_blob_gas(parent_excess_blob_gas, parent_blob_gas_used))
            } else {
                // for the first post-fork block, both parent.blob_gas_used and
                // parent.excess_blob_gas are evaluated as 0
                Some(calculate_excess_blob_gas(0, 0))
            };

            blob_gas_used = Some(0);
        }

        let header = Header {
            parent_hash: parent_block.hash(),
            ommers_hash: EMPTY_OMMER_ROOT_HASH,
            beneficiary: initialized_block_env.coinbase,
            state_root,
            transactions_root: EMPTY_TRANSACTIONS,
            withdrawals_root,
            receipts_root: EMPTY_RECEIPTS,
            logs_bloom: Default::default(),
            timestamp: attributes.timestamp(),
            mix_hash: attributes.prev_randao(),
            nonce: BEACON_NONCE,
            base_fee_per_gas: Some(base_fee),
            number: parent_block.number + 1,
            gas_limit: block_gas_limit,
            difficulty: U256::ZERO,
            gas_used: 0,
            extra_data,
            blob_gas_used,
            excess_blob_gas,
            parent_beacon_block_root: attributes.parent_beacon_block_root(),
            requests_root,
        };

        let block = Block { header, body: vec![], ommers: vec![], withdrawals, requests };
        let sealed_block = block.seal_slow();

        Ok(EthBuiltPayload::new(attributes.payload_id(), sealed_block, U256::ZERO))
    }
}

/// Constructs an Ethereum transaction payload using the best transactions from the pool.
///
/// Given build arguments including an Ethereum client, transaction pool,
/// and configuration, this function creates a transaction payload. Returns
/// a result indicating success with the payload or an error in case of failure.
#[inline]
pub fn default_ethereum_payload_builder<Pool, Client>(
    evm_config: EthEvmConfig,
    args: BuildArguments<Pool, Client, BuilderPayloadBuilderAttributes, EthBuiltPayload>,
) -> Result<(BuildOutcome<EthBuiltPayload>, Option<BundleStateWithReceipts>), PayloadBuilderError>
where
    Client: StateProviderFactory,
    Pool: TransactionPool,
{
    let BuildArguments { client, pool, mut cached_reads, config, cancel, best_payload } = args;

    let state_provider = client.state_by_block_hash(config.parent_block.hash())?;
    let state = StateProviderDatabase::new(&state_provider);
    let mut db =
        State::builder().with_database_ref(cached_reads.as_db(&state)).with_bundle_update().build();
    let extra_data = config.extra_data();
    let PayloadConfig {
        initialized_block_env,
        initialized_cfg,
        parent_block,
        attributes,
        chain_spec,
        ..
    } = config;

    debug!(target: "payload_builder", id=%attributes.payload_id(), parent_hash = ?parent_block.hash(), parent_number = parent_block.number, "building new payload");
    let mut cumulative_gas_used = 0;
    let mut sum_blob_gas_used = 0;
    let block_gas_limit: u64 = initialized_block_env.gas_limit.try_into().unwrap_or(u64::MAX);
    let base_fee = initialized_block_env.basefee.to::<u64>();

    let mut executed_txs = Vec::new();

    let mut best_txs = pool.best_transactions_with_attributes(BestTransactionsAttributes::new(
        base_fee,
        initialized_block_env.get_blob_gasprice().map(|gasprice| gasprice as u64),
    ));

    let mut total_fees = U256::ZERO;

    let block_number = initialized_block_env.number.to::<u64>();

    // apply eip-4788 pre block contract call
    pre_block_beacon_root_contract_call(
        &mut db,
        &chain_spec,
        block_number,
        &initialized_cfg,
        &initialized_block_env,
        &attributes,
    )?;

    // apply eip-2935 blockhashes update
    apply_blockhashes_update(
        &chain_spec,
        initialized_block_env.timestamp.to::<u64>(),
        block_number,
        &mut db,
    )
    .map_err(|err| PayloadBuilderError::Internal(err.into()))?;

    let mut receipts = Vec::new();
    while let Some(pool_tx) = best_txs.next() {
        // ensure we still have capacity for this transaction
        if cumulative_gas_used + pool_tx.gas_limit() > block_gas_limit {
            // we can't fit this transaction into the block, so we need to mark it as invalid
            // which also removes all dependent transaction from the iterator before we can
            // continue
            best_txs.mark_invalid(&pool_tx);
            continue
        }

        // check if the job was cancelled, if so we can exit early
        if cancel.is_cancelled() {
            return Ok((BuildOutcome::Cancelled, None))
        }

        // convert tx to a signed transaction
        let tx = pool_tx.to_recovered_transaction();

        // There's only limited amount of blob space available per block, so we need to check if
        // the EIP-4844 can still fit in the block
        if let Some(blob_tx) = tx.transaction.as_eip4844() {
            let tx_blob_gas = blob_tx.blob_gas();
            if sum_blob_gas_used + tx_blob_gas > MAX_DATA_GAS_PER_BLOCK {
                // we can't fit this _blob_ transaction into the block, so we mark it as
                // invalid, which removes its dependent transactions from
                // the iterator. This is similar to the gas limit condition
                // for regular transactions above.
                trace!(target: "payload_builder", tx=?tx.hash, ?sum_blob_gas_used, ?tx_blob_gas, "skipping blob transaction because it would exceed the max data gas per block");
                best_txs.mark_invalid(&pool_tx);
                continue
            }
        }

        let env = EnvWithHandlerCfg::new_with_cfg_env(
            initialized_cfg.clone(),
            initialized_block_env.clone(),
            tx_env_with_recovered(&tx),
        );

        // Configure the environment for the block.
        let mut evm = evm_config.evm_with_env(&mut db, env);

        let ResultAndState { result, state } = match evm.transact() {
            Ok(res) => res,
            Err(err) => {
                match err {
                    EVMError::Transaction(err) => {
                        if matches!(err, InvalidTransaction::NonceTooLow { .. }) {
                            // if the nonce is too low, we can skip this transaction
                            trace!(target: "payload_builder", %err, ?tx, "skipping nonce too low transaction");
                        } else {
                            // if the transaction is invalid, we can skip it and all of its
                            // descendants
                            trace!(target: "payload_builder", %err, ?tx, "skipping invalid transaction and its descendants");
                            best_txs.mark_invalid(&pool_tx);
                        }

                        continue
                    }
                    err => {
                        // this is an error that we should treat as fatal for this attempt
                        return Err(PayloadBuilderError::EvmExecutionError(err))
                    }
                }
            }
        };
        // drop evm so db is released.
        drop(evm);
        // commit changes
        db.commit(state);

        // add to the total blob gas used if the transaction successfully executed
        if let Some(blob_tx) = tx.transaction.as_eip4844() {
            let tx_blob_gas = blob_tx.blob_gas();
            sum_blob_gas_used += tx_blob_gas;

            // if we've reached the max data gas per block, we can skip blob txs entirely
            if sum_blob_gas_used == MAX_DATA_GAS_PER_BLOCK {
                best_txs.skip_blobs();
            }
        }

        let gas_used = result.gas_used();

        // add gas used by the transaction to cumulative gas used, before creating the receipt
        cumulative_gas_used += gas_used;

        // Push transaction changeset and calculate header bloom filter for receipt.
        #[allow(clippy::needless_update)] // side-effect of optimism fields
        receipts.push(Some(Receipt {
            tx_type: tx.tx_type(),
            success: result.is_success(),
            cumulative_gas_used,
            logs: result.into_logs().into_iter().map(Into::into).collect(),
            ..Default::default()
        }));

        // update add to total fees
        let miner_fee = tx
            .effective_tip_per_gas(Some(base_fee))
            .expect("fee is always valid; execution succeeded");
        total_fees += U256::from(miner_fee) * U256::from(gas_used);

        // append transaction to the list of executed transactions
        executed_txs.push(tx.into_signed());
    }

    // check if we have a better block
    if !is_better_payload(best_payload.as_ref(), total_fees) {
        // can skip building the block
        return Ok((BuildOutcome::Aborted { fees: total_fees, cached_reads }, None))
    }

    // calculate the requests and the requests root
    let (requests, requests_root) = if chain_spec
        .is_prague_active_at_timestamp(attributes.timestamp())
    {
        let deposit_requests = parse_deposits_from_receipts(&chain_spec, receipts.iter().flatten())
            .map_err(|err| PayloadBuilderError::Internal(RethError::Execution(err.into())))?;

        let withdrawal_requests = post_block_withdrawal_requests_contract_call(
            &mut db,
            &chain_spec,
            &initialized_cfg,
            &initialized_block_env,
            &attributes,
        )?;

        let requests = [deposit_requests, withdrawal_requests].concat();
        let requests_root = calculate_requests_root(&requests);
        (Some(requests.into()), Some(requests_root))
    } else {
        (None, None)
    };

    let WithdrawalsOutcome { withdrawals_root, withdrawals } = commit_withdrawals(
        &mut db,
        &chain_spec,
        attributes.timestamp(),
        attributes.withdrawals().clone(),
    )?;

    // merge all transitions into bundle state, this would apply the withdrawal balance changes
    // and 4788 contract call
    db.merge_transitions(BundleRetention::PlainState);

    let bundle = BundleStateWithReceipts::new(
        db.take_bundle(),
        Receipts::from_vec(vec![receipts]),
        block_number,
    );
    let receipts_root = bundle.receipts_root_slow(block_number).expect("Number is in range");
    let logs_bloom = bundle.block_logs_bloom(block_number).expect("Number is in range");

    // calculate the state root
    let state_root = {
        let state_provider = db.database.0.inner.borrow_mut();
        state_provider.db.state_root(bundle.state())?
    };

    // create the block header
    let transactions_root = proofs::calculate_transaction_root(&executed_txs);

    // initialize empty blob sidecars at first. If cancun is active then this will
    let mut blob_sidecars = Vec::new();
    let mut excess_blob_gas = None;
    let mut blob_gas_used = None;

    // only determine cancun fields when active
    if chain_spec.is_cancun_active_at_timestamp(attributes.timestamp()) {
        // grab the blob sidecars from the executed txs
        blob_sidecars = pool.get_all_blobs_exact(
            executed_txs.iter().filter(|tx| tx.is_eip4844()).map(|tx| tx.hash).collect(),
        )?;

        excess_blob_gas = if chain_spec.is_cancun_active_at_timestamp(parent_block.timestamp) {
            let parent_excess_blob_gas = parent_block.excess_blob_gas.unwrap_or_default();
            let parent_blob_gas_used = parent_block.blob_gas_used.unwrap_or_default();
            Some(calculate_excess_blob_gas(parent_excess_blob_gas, parent_blob_gas_used))
        } else {
            // for the first post-fork block, both parent.blob_gas_used and
            // parent.excess_blob_gas are evaluated as 0
            Some(calculate_excess_blob_gas(0, 0))
        };

        blob_gas_used = Some(sum_blob_gas_used);
    }

    let header = Header {
        parent_hash: parent_block.hash(),
        ommers_hash: EMPTY_OMMER_ROOT_HASH,
        beneficiary: initialized_block_env.coinbase,
        state_root,
        transactions_root,
        receipts_root,
        withdrawals_root,
        logs_bloom,
        timestamp: attributes.timestamp(),
        mix_hash: attributes.prev_randao(),
        nonce: BEACON_NONCE,
        base_fee_per_gas: Some(base_fee),
        number: parent_block.number + 1,
        gas_limit: block_gas_limit,
        difficulty: U256::ZERO,
        gas_used: cumulative_gas_used,
        extra_data,
        parent_beacon_block_root: attributes.parent_beacon_block_root(),
        blob_gas_used,
        excess_blob_gas,
        requests_root,
    };

    // seal the block
    let block = Block { header, body: executed_txs, ommers: vec![], withdrawals, requests };

    let sealed_block = block.seal_slow();
    debug!(target: "payload_builder", ?sealed_block, "sealed built block");

    let mut payload = EthBuiltPayload::new(attributes.payload_id(), sealed_block, total_fees);

    // extend the payload with the blob sidecars from the executed txs
    payload.extend_sidecars(blob_sidecars);

    Ok((BuildOutcome::Better { payload, cached_reads }, Some(bundle)))
}
