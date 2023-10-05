#![allow(unused_imports)]
use async_trait::async_trait;

use std::{collections::HashMap, sync::Arc};

use ethereum_consensus::primitives::BlsPublicKey;
use mev_rs::{
    types::{SignedBidSubmission, ValidationStatus},
    Error,
};
pub type ValidatorPreferences = HashMap<BlsPublicKey, SignedBidSubmission>;
use jsonrpsee::{core::RpcResult, proc_macros::rpc};
use reth::{
    cli::{
        config::RethRpcConfig,
        ext::{RethCliExt, RethNodeCommandConfig},
        Cli,
    },
    network::{NetworkInfo, Peers},
    providers::{
        AccountReader, BlockReaderIdExt, CanonStateSubscriptions, ChainSpecProvider,
        ChangeSetReader, EvmEnvProvider, HeaderProvider, ProviderError, StateProviderFactory,
        WithdrawalsProvider,
    },
    rpc::{
        builder::{RethModuleRegistry, TransportRpcModules},
        types::engine::ExecutionPayload,
    },
    tasks::TaskSpawner,
    transaction_pool::TransactionPool,
};

use crate::types::ValidationRequestBody;

/// Payloadverifier ext
pub struct PayloadValidationExt;

impl RethCliExt for PayloadValidationExt {
    type Node = RethCliValidationExt;
}

#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub struct RethCliValidationExt {
    #[clap(long)]
    pub enable_ext: bool,
}

impl RethNodeCommandConfig for RethCliValidationExt {
    fn extend_rpc_modules<Conf, Provider, Pool, Network, Tasks, Events>(
        &mut self,
        _config: &Conf,
        registry: &mut RethModuleRegistry<Provider, Pool, Network, Tasks, Events>,
        modules: &mut TransportRpcModules,
    ) -> eyre::Result<()>
    where
        Conf: RethRpcConfig,
        Provider: BlockReaderIdExt
            + StateProviderFactory
            + EvmEnvProvider
            + ChainSpecProvider
            + ChangeSetReader
            + Clone
            + Unpin
            + 'static,
        Pool: TransactionPool + Clone + 'static,
        Network: NetworkInfo + Peers + Clone + 'static,
        Tasks: TaskSpawner + Clone + 'static,
        Events: CanonStateSubscriptions + Clone + 'static,
    {
        if !self.enable_ext {
            return Ok(())
        }

        let provider = registry.provider().clone();
        let ext = ValidationExt { provider };

        modules.merge_configured(ext.into_rpc())?;

        println!("Payload Verification extension enabled");
        Ok(())
    }
}

#[rpc(server, namespace = "validationExt")]
#[async_trait]
pub trait ValidationExtApi {
    #[method(name = "validate_payload")]
    async fn validate_payload(&self, payload: &ValidationRequestBody) -> RpcResult<()>;
}

#[async_trait]
impl<Provider> ValidationExtApiServer for ValidationExt<Provider>
where
    Provider: BlockReaderIdExt
        + ChainSpecProvider
        + ChangeSetReader
        + StateProviderFactory
        + HeaderProvider
        + AccountReader
        + WithdrawalsProvider
        + 'static,
{
    async fn validate_payload(&self, payload: &ValidationRequestBody) -> RpcResult<()> {
        todo!()
    }
}

pub struct ValidationExt<Provider> {
    provider: Provider,
}
