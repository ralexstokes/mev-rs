use crate::reth_builder::{service::Service, Config, DeadlineBidder};
use clap::{Args, Parser};
use ethereum_consensus::{
    networks::{self},
    state_transition::Context,
};
use reth::{
    cli::ext::{RethCliExt, RethNodeCommandConfig},
    node::NodeCommand,
    runner::CliContext,
    tasks::TaskManager,
};
use reth_payload_builder::PayloadBuilderService;
use std::{sync::Arc, time::Duration};
use tracing::warn;

#[derive(Debug, Args)]
pub struct ServiceExt {
    #[clap(skip)]
    config: Config,
}

impl ServiceExt {
    pub fn from(config: Config) -> Self {
        Self { config }
    }

    pub async fn spawn(self) {
        let task_manager = TaskManager::new(tokio::runtime::Handle::current());
        let task_executor = task_manager.executor();
        let ctx = CliContext { task_executor };

        let network = &self.config.network;
        let network_name = format!("{0}", network);

        let mut params =
            vec!["".into(), "--chain".into(), network_name.to_string(), "--http".into()];
        if let Some(path) = self.config.jwt_secret_path.as_ref() {
            params.push("--authrpc.jwtsecret".into());
            params.push(path.clone());
        }

        let mut node = NodeCommand::<ServiceExt>::parse_from(params);
        // NOTE: shim to pass in config
        node.ext = self;
        if let Err(err) = node.execute(ctx).await {
            warn!("{err:?}");
        }
    }
}

impl RethCliExt for ServiceExt {
    type Node = ServiceExt;
}

impl RethNodeCommandConfig for ServiceExt {
    fn spawn_payload_builder_service<Conf, Provider, Pool, Tasks>(
        &mut self,
        _conf: &Conf,
        provider: Provider,
        pool: Pool,
        executor: Tasks,
        chain_spec: std::sync::Arc<reth_primitives::ChainSpec>,
    ) -> eyre::Result<reth_payload_builder::PayloadBuilderHandle>
    where
        Conf: reth::cli::config::PayloadBuilderConfig,
        Provider: reth::providers::StateProviderFactory
            + reth::providers::BlockReaderIdExt
            + Clone
            + Unpin
            + 'static,
        Pool: reth::transaction_pool::TransactionPool + Unpin + 'static,
        Tasks: reth::tasks::TaskSpawner + Clone + Unpin + 'static,
    {
        let build_config = self.config.clone();
        let network = &build_config.network;
        let context = Arc::new(Context::try_from(network)?);
        let clock = context.clock().unwrap_or_else(|| {
            let genesis_time = networks::typical_genesis_time(&context);
            context.clock_at(genesis_time)
        });
        let deadline = Duration::from_millis(build_config.bidding_deadline_ms);
        let bidder = Arc::new(DeadlineBidder::new(clock.clone(), deadline));
        let (service, builder) = Service::from(
            build_config,
            context,
            clock,
            pool.clone(),
            provider.clone(),
            bidder,
            chain_spec.clone(),
        )
        .unwrap();

        let (payload_service, payload_builder) = PayloadBuilderService::new(builder);

        let fut = Box::pin(async move {
            match service.spawn().await {
                Ok(handle) => match handle.await {
                    Ok(()) => (),
                    Err(err) => {
                        tracing::error!(err = %err, "error awaiting builder service");
                    }
                },
                Err(err) => {
                    tracing::error!(err = %err, "could not launch builder");
                }
            }
        });

        executor.spawn_critical("boost builder", fut);
        executor.spawn_critical("payload builder service", Box::pin(payload_service));

        Ok(payload_builder)
    }
}
