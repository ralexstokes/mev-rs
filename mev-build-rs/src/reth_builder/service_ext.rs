use crate::reth_builder::{service::Service, Config, DeadlineBidder};
use clap::{Args, Parser};
use ethereum_consensus::{
    networks::{self, Network},
    state_transition::Context,
};
use reth::{
    cli::{
        components::RethNodeComponents,
        config::PayloadBuilderConfig,
        ext::{RethCliExt, RethNodeCommandConfig},
    },
    node::NodeCommand,
    runner::CliContext,
    tasks::{TaskManager, TaskSpawner},
};
use reth_payload_builder::{PayloadBuilderHandle, PayloadBuilderService};
use std::{sync::Arc, time::Duration};
use tracing::warn;

#[derive(Debug, Args)]
pub struct ServiceExt {
    #[clap(skip)]
    network: Network,
    #[clap(skip)]
    config: Config,
}

impl ServiceExt {
    pub fn from(network: Network, config: Config) -> Self {
        Self { network, config }
    }

    pub async fn spawn(self) {
        let task_manager = TaskManager::new(tokio::runtime::Handle::current());
        let task_executor = task_manager.executor();
        let ctx = CliContext { task_executor };

        let network = &self.network;
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
    fn spawn_payload_builder_service<Conf, Reth>(
        &mut self,
        _conf: &Conf,
        components: &Reth,
    ) -> eyre::Result<PayloadBuilderHandle>
    where
        Conf: PayloadBuilderConfig,
        Reth: RethNodeComponents,
    {
        let build_config = self.config.clone();
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
            components.pool(),
            components.provider(),
            bidder,
            components.chain_spec(),
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

        components.task_executor().spawn_critical("boost builder", fut);
        components
            .task_executor()
            .spawn_critical("payload builder service", Box::pin(payload_service));

        Ok(payload_builder)
    }
}
