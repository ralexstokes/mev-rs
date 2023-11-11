use crate::reth_builder::{service::Service, Config as BuildConfig, DeadlineBidder};
use clap::Args;
use ethereum_consensus::{
    networks::{self, Network},
    state_transition::Context,
};
use mev_rs::config::from_toml_file;
use reth::{
    cli::{
        components::RethNodeComponents,
        config::PayloadBuilderConfig,
        ext::{RethCliExt, RethNodeCommandConfig},
    },
    tasks::TaskSpawner,
};
use reth_payload_builder::{PayloadBuilderHandle, PayloadBuilderService};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Args)]
pub struct ServiceExt {
    #[clap(env, long = "mev-builder-config", default_value = "config.toml")]
    config_file: String,
    #[clap(skip)]
    config: Option<Config>,
}

// NOTE: this is duplicated here to avoid circular import b/t `mev` bin and `mev-rs` crate
#[derive(Debug, serde::Deserialize)]
struct Config {
    pub network: Network,
    #[serde(rename = "builder")]
    pub build: BuildConfig,
}

impl RethCliExt for ServiceExt {
    type Node = ServiceExt;
}

impl RethNodeCommandConfig for ServiceExt {
    fn on_components_initialized<Reth: RethNodeComponents>(
        &mut self,
        _components: &Reth,
    ) -> eyre::Result<()> {
        let config_file = &self.config_file;

        let config = from_toml_file::<_, Config>(config_file)?;
        let network = &config.network;
        tracing::info!("configured for `{network}`");

        self.config = Some(config);
        Ok(())
    }

    fn spawn_payload_builder_service<Conf, Reth>(
        &mut self,
        _conf: &Conf,
        components: &Reth,
    ) -> eyre::Result<PayloadBuilderHandle>
    where
        Conf: PayloadBuilderConfig,
        Reth: RethNodeComponents,
    {
        let config = self.config.as_ref().ok_or(eyre::eyre!("already loaded config"))?;
        let context = Arc::new(Context::try_from(config.network.clone())?);
        let clock = context.clock().unwrap_or_else(|| {
            let genesis_time = networks::typical_genesis_time(&context);
            context.clock_at(genesis_time)
        });
        let build_config = &config.build;
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
        .map_err(|err| eyre::eyre!(err))?;

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
