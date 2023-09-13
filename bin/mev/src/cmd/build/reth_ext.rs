use crate::cmd::config::Config;
use clap::{Args, Parser};
use ethereum_consensus::{networks, networks::Network, state_transition::Context};
use mev_build_rs::reth_builder::{Config as BuildConfig, DeadlineBidder, Service};
use reth::{
    cli::ext::{RethCliExt, RethNodeCommandConfig},
    node::NodeCommand,
    runner::CliContext,
    tasks::TaskManager,
};
use reth_payload_builder::PayloadBuilderService;
use std::{sync::Arc, time::Duration};

struct RethExt;

impl RethCliExt for RethExt {
    type Node = RethNodeExt;
}

#[derive(Debug, Args)]
pub struct RethNodeExt {
    #[clap(skip)]
    pub config_file: String,
    #[clap(skip)]
    pub network: Network,
    #[clap(skip)]
    pub config: Option<BuildConfig>,
}

impl RethNodeExt {
    pub fn get_build_config(&mut self) -> BuildConfig {
        self.config.take().unwrap_or_else(|| {
            let config = Config::from_toml_file(&self.config_file).unwrap();
            let config = config.build.unwrap();
            self.config = Some(config.clone());
            config
        })
    }
}

impl RethNodeCommandConfig for RethNodeExt {
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
        let build_config = self.get_build_config();
        let network = &self.network;
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

pub(crate) async fn launch_reth_with(mut ext: RethNodeExt) {
    let task_manager = TaskManager::new(tokio::runtime::Handle::current());
    let task_executor = task_manager.executor();
    let ctx = CliContext { task_executor };

    let config = ext.get_build_config();

    let network_name = format!("{0}", ext.network);

    let mut params = vec!["".into(), "--chain".into(), network_name.to_string(), "--http".into()];
    if let Some(path) = config.jwt_secret_path {
        params.push("--authrpc.jwtsecret".into());
        params.push(path);
    }

    let mut node = NodeCommand::<RethExt>::parse_from(params);
    // NOTE: shim to pass in config
    node.ext = ext;
    if let Err(err) = node.execute(ctx).await {
        tracing::warn!("{err:?}");
    }
}
