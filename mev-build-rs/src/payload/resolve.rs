//! Resolve a given payload for use in the auction
//! Takes a payload from the payload builder and "finalizes" the crafted payload to yield a valid
//! block according to the auction rules.

use crate::{payload::builder::PayloadBuilder, utils::payload_job::ResolveBestPayload};
use alloy_signer::SignerSync;
use alloy_signer_wallet::LocalWallet;
use futures_util::FutureExt;
use reth::{
    payload::{error::PayloadBuilderError, EthBuiltPayload, PayloadId},
    primitives::{
        kzg::{Blob, Bytes48},
        proofs,
        revm::env::tx_env_with_recovered,
        Address, BlobTransactionSidecar, Block, ChainId, Receipt, SealedBlock, Signature,
        Transaction, TransactionKind, TransactionSigned, TransactionSignedEcRecovered, TxEip1559,
        B256, U256,
    },
    providers::{BundleStateWithReceipts, StateProviderFactory},
    revm::{
        self,
        database::StateProviderDatabase,
        db::states::bundle_state::BundleRetention,
        primitives::{BlockEnv, CfgEnvWithHandlerCfg, EnvWithHandlerCfg, ResultAndState},
        DatabaseCommit, State,
    },
    rpc::types::engine::{BlobsBundleV1, ExecutionPayloadEnvelopeV3},
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
        to: TransactionKind::Call(config.proposer_fee_recipient),
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
    // TODO: can we get some kind of pending state against `block.hash` here instead of replaying
    // the bundle state?
    let state_provider = client.state_by_block_hash(config.parent_hash)?;
    let state = StateProviderDatabase::new(&state_provider);
    let bundle_state_with_receipts = config
        .builder
        .get_build_state(config.payload_id)
        .ok_or_else(|| PayloadBuilderError::Other("missing build state for payload".into()))?;
    // TODO: use cached reads
    let mut db = State::builder()
        .with_database_ref(state)
        // TODO skip clone here...
        .with_bundle_prestate(bundle_state_with_receipts.state().clone())
        .with_bundle_update()
        .build();

    let signer_account = db.load_cache_account(config.sender)?;
    // TODO handle option
    let nonce = signer_account.account_info().expect("account exists").nonce;
    // TODO handle option
    let max_fee_per_gas = block.header().base_fee_per_gas.expect("exists") as u128;
    let payment_tx = make_payment_transaction(config, nonce, max_fee_per_gas, value)?;

    // === Apply txn ===

    // TODO: skip clones here
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
    // TODO: sanity check we didn't go over gas limit
    let cumulative_gas_used = header.gas_used + result.gas_used();
    let receipt = Receipt {
        tx_type: payment_tx.tx_type(),
        success: result.is_success(),
        cumulative_gas_used,
        logs: result.into_logs().into_iter().map(Into::into).collect(),
    };
    // TODO skip clone here
    let mut receipts = bundle_state_with_receipts.receipts().clone();
    receipts.push(vec![Some(receipt)]);

    body.push(payment_tx.into_signed());

    db.merge_transitions(BundleRetention::PlainState);

    let block_number = header.number;
    let bundle = BundleStateWithReceipts::new(db.take_bundle(), receipts, block_number);

    let receipts_root = bundle.receipts_root_slow(block_number).expect("Number is in range");
    let logs_bloom = bundle.block_logs_bloom(block_number).expect("Number is in range");
    let state_root = state_provider.state_root(bundle.state())?;
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
    pub payload_id: PayloadId,
    pub proposer_fee_recipient: Address,
    pub signer: Arc<LocalWallet>,
    pub sender: Address,
    pub parent_hash: B256,
    pub chain_id: ChainId,
    pub cfg_env: CfgEnvWithHandlerCfg,
    pub block_env: BlockEnv,
    pub builder: PayloadBuilder,
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
        // TODO: remove temporary hardcoded subsidy
        fees.max(U256::from(1337))
    }

    fn prepare(
        &self,
        block: SealedBlock,
        fees: U256,
        config: &PayloadFinalizerConfig,
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        let payment_amount = self.determine_payment_amount(fees);
        let block = append_payment(&self.client, config, block, payment_amount)?;
        // TODO: - track proposer payment, revenue
        // TODO: ensure fees haven't changed
        Ok(EthBuiltPayload::new(self.payload_id, block, payment_amount))
    }

    fn process(
        &mut self,
        block: SealedBlock,
        fees: U256,
    ) -> Result<EthBuiltPayload, PayloadBuilderError> {
        if let Some(config) = self.config.as_ref() {
            self.prepare(block, fees, config)
        } else {
            Ok(EthBuiltPayload::new(self.payload_id, block, fees))
        }
    }
}

#[derive(Debug)]
pub struct ResolveBuilderPayload<Client, Pool> {
    pub resolution: ResolveBestPayload<EthBuiltPayload>,
    pub finalizer: PayloadFinalizer<Client, Pool>,
}

impl<Client, Pool> Future for ResolveBuilderPayload<Client, Pool>
where
    Client: StateProviderFactory + Unpin,
    Pool: Unpin,
{
    type Output = Result<EthBuiltPayload, PayloadBuilderError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let payload = ready!(this.resolution.poll_unpin(cx))?;

        // TODO: consider making the payment addition `spawn_blocking`
        // TODO: save payload in the event we need to poll again?

        // TODO: we are dropping blobs....

        let block = payload.block().clone();
        let fees = payload.fees();

        // TODO: move to custom type to skip copy on blobs
        // NOTE: workaround, can move to our own type to skip all this copying
        let execution_payload = ExecutionPayloadEnvelopeV3::from(payload);

        let BlobsBundleV1 { commitments, proofs, blobs } = execution_payload.blobs_bundle;
        let blob_sidecars = BlobTransactionSidecar {
            blobs: blobs
                .into_iter()
                .map(|blob| Blob::from_bytes(blob.as_ref()).expect("is right size"))
                .collect(),
            commitments: commitments
                .into_iter()
                .map(|c| Bytes48::from_bytes(c.as_ref()).expect("is right size"))
                .collect(),
            proofs: proofs
                .into_iter()
                .map(|p| Bytes48::from_bytes(p.as_ref()).expect("is right size"))
                .collect(),
        };

        let finalized_payload = this.finalizer.process(block, fees).map(|mut payload| {
            payload.extend_sidecars(vec![blob_sidecars]);
            payload
        });
        Poll::Ready(finalized_payload)
    }
}
