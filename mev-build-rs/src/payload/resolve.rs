//! Resolve a given payload for use in the auction
//! Takes a payload from the payload builder and "finalizes" the crafted payload to yield a valid
//! block according to the auction rules.

use crate::utils::payload_job::ResolveBestPayload;
use alloy_signer::SignerSync;
use alloy_signer_wallet::LocalWallet;
use futures_util::FutureExt;
use reth::{
    api::BuiltPayload,
    payload::{error::PayloadBuilderError, EthBuiltPayload, PayloadId},
    primitives::{
        proofs, revm::env::tx_env_with_recovered, Address, Block, ChainId, Receipt, Receipts,
        SealedBlock, Signature, Transaction, TransactionKind, TransactionSigned,
        TransactionSignedEcRecovered, TxEip1559, B256, U256,
    },
    providers::{BundleStateWithReceipts, StateProviderFactory},
    revm::{
        self,
        database::StateProviderDatabase,
        db::states::bundle_state::BundleRetention,
        primitives::{BlockEnv, CfgEnvWithHandlerCfg, EnvWithHandlerCfg, ResultAndState},
        DatabaseCommit, State,
    },
};
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{ready, Context, Poll},
};

pub const BASE_TX_GAS_LIMIT: u64 = 21000;

fn make_payment_transaction(
    config: &PayloadFinalizerConfig,
    nonce: u64,
    max_fee_per_gas: u128,
    value: U256,
) -> Result<TransactionSignedEcRecovered, PayloadBuilderError> {
    let tx = Transaction::Eip1559(TxEip1559 {
        chain_id: config.chain_id,
        nonce,
        gas_limit: BASE_TX_GAS_LIMIT,
        // SAFETY: cast to bigger type always succeeds
        max_fee_per_gas,
        max_priority_fee_per_gas: 0,
        to: TransactionKind::Call(config.sender),
        value,
        access_list: Default::default(),
        input: Default::default(),
    });
    // TODO: verify we are signing correctly...
    let signature_hash = tx.signature_hash();
    let signature = config.signer.sign_hash_sync(&signature_hash).expect("can sign");
    let signed_transaction = TransactionSigned::from_transaction_and_signature(
        tx,
        Signature { r: signature.r(), s: signature.s(), odd_y_parity: signature.v().y_parity() },
    );
    Ok(TransactionSignedEcRecovered::from_signed_transaction(signed_transaction, config.sender))
}

fn append_payment<Client: StateProviderFactory>(
    client: &Client,
    config: &PayloadFinalizerConfig,
    block: SealedBlock,
    value: U256,
) -> Result<SealedBlock, PayloadBuilderError> {
    let state_provider = client.state_by_block_hash(config.parent_hash)?;
    let state = StateProviderDatabase::new(&state_provider);
    // TODO: use cached reads
    let mut db = State::builder().with_database_ref(state).with_bundle_update().build();

    let signer_account = db.load_cache_account(config.sender)?;
    // TODO handle option
    let nonce = signer_account.account_info().expect("account exists").nonce;
    // TODO handle option
    let max_fee_per_gas = block.header().base_fee_per_gas.expect("exists") as u128;
    let payment_tx = make_payment_transaction(config, nonce, max_fee_per_gas, value)?;

    // === Apply txn ===

    // TODO: try to clone the envs less here...
    let env = EnvWithHandlerCfg::new_with_cfg_env(
        config.cfg_env.clone(),
        config.block_env.clone(),
        tx_env_with_recovered(&payment_tx),
    );
    let mut evm = revm::Evm::builder().with_db(&mut db).with_env_with_handler_cfg(env).build();

    let ResultAndState { result, state } =
        evm.transact().map_err(PayloadBuilderError::EvmExecutionError)?;

    drop(evm);
    db.commit(state);

    let Block { mut header, mut body, ommers, withdrawals } = block.unseal();

    // TODO: hold gas reserve so this always succeeds
    let cumulative_gas_used = header.gas_used + result.gas_used();
    // TODO: sanity check we didn't go over gas limit
    let receipt = Receipt {
        tx_type: payment_tx.tx_type(),
        success: result.is_success(),
        cumulative_gas_used,
        logs: result.into_logs().into_iter().map(Into::into).collect(),
        ..Default::default()
    };

    body.push(payment_tx.into_signed());

    db.merge_transitions(BundleRetention::PlainState);

    let block_number = header.number;
    // TODO: this is broken bc we need to keep receipts...
    // NOTE: need to either fetch receipts from DB, or just keep state and not build in two steps
    // here...
    // May want to pass bundle state from payload builder and then `extend` here instead...
    let bundle = BundleStateWithReceipts::new(
        db.take_bundle(),
        Receipts::from_vec(vec![vec![Some(receipt)]]),
        block_number,
    );

    let receipts_root = bundle.receipts_root_slow(block_number).expect("Number is in range");
    let logs_bloom = bundle.block_logs_bloom(block_number).expect("Number is in range");

    // calculate the state root
    let state_root = state_provider.state_root(bundle.state())?;

    // create the block header
    let transactions_root = proofs::calculate_transaction_root(&body);

    header.state_root = state_root;
    header.transactions_root = transactions_root;
    header.receipts_root = receipts_root;
    header.logs_bloom = logs_bloom;
    header.gas_used = cumulative_gas_used;

    let block = Block { header, body, ommers, withdrawals };

    Ok(block.seal_slow())
}

#[derive(Debug)]
pub struct PayloadFinalizerConfig {
    pub proposer_fee_recipient: Address,
    pub signer: Arc<LocalWallet>,
    pub sender: Address,
    pub parent_hash: B256,
    pub chain_id: ChainId,
    pub cfg_env: CfgEnvWithHandlerCfg,
    pub block_env: BlockEnv,
}

#[derive(Debug)]
pub struct PayloadFinalizer<Client, Pool> {
    pub client: Client,
    pub _pool: Pool,
    pub payload_id: PayloadId,
    pub config: Option<PayloadFinalizerConfig>,
}

impl<Client: StateProviderFactory, Pool> PayloadFinalizer<Client, Pool> {
    fn determine_payment_amount(&self, fees: U256) -> U256 {
        // TODO: get amount to bid from bidder
        // - amount from block fees
        // - including any subsidy
        fees
    }

    fn prepare(
        &self,
        block: &SealedBlock,
        fees: U256,
        config: &PayloadFinalizerConfig,
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        let payment_amount = self.determine_payment_amount(fees);
        let block = append_payment(&self.client, config, block.clone(), payment_amount)?;
        // TODO: - track proposer payment, revenue
        Ok(EthBuiltPayload::new(self.payload_id, block, fees))
    }

    fn process(
        &mut self,
        block: &SealedBlock,
        fees: U256,
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        if let Some(config) = self.config.as_ref() {
            self.prepare(block, fees, config)
        } else {
            Ok(EthBuiltPayload::new(self.payload_id, block.clone(), fees))
        }
    }
}

#[derive(Debug)]
pub struct ResolveBuilderPayload<Payload, Client, Pool> {
    pub resolution: ResolveBestPayload<Payload>,
    pub finalizer: PayloadFinalizer<Client, Pool>,
}

impl<Payload, Client, Pool> Future for ResolveBuilderPayload<Payload, Client, Pool>
where
    Payload: BuiltPayload + Unpin,
    Client: StateProviderFactory + Unpin,
    Pool: Unpin,
{
    type Output = Result<EthBuiltPayload, PayloadBuilderError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let payload = ready!(this.resolution.poll_unpin(cx))?;

        // TODO: save payload in the event we need to poll again?

        // TODO: we are dropping blobs....

        let block = payload.block();
        let fees = payload.fees();

        let finalized_payload = this.finalizer.process(block, fees);
        Poll::Ready(finalized_payload)
    }
}
