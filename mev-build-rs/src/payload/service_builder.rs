use crate::{
    node::BuilderEngineTypes,
    payload::{
        builder::PayloadBuilder,
        job_generator::{PayloadJobGenerator, PayloadJobGeneratorConfig},
    },
};
use reth::{
    builder::{node::FullNodeTypes, BuilderContext},
    cli::config::PayloadBuilderConfig,
    payload::{PayloadBuilderHandle, PayloadBuilderService},
    primitives::Bytes,
    providers::CanonStateSubscriptions,
    transaction_pool::TransactionPool,
};

#[derive(Debug, Clone, Default)]
pub struct PayloadServiceBuilder {
    pub extra_data: Option<Bytes>,
}

impl<Node, Pool> reth::builder::components::PayloadServiceBuilder<Node, Pool>
    for PayloadServiceBuilder
where
    Node: FullNodeTypes<Engine = BuilderEngineTypes>,
    Pool: TransactionPool + Unpin + 'static,
{
    async fn spawn_payload_service(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
    ) -> eyre::Result<PayloadBuilderHandle<Node::Engine>> {
        let conf = ctx.payload_builder_config();

        let extradata = if let Some(extra_data) = self.extra_data {
            extra_data
        } else {
            conf.extradata_bytes()
        };
        let payload_job_config = PayloadJobGeneratorConfig {
            extradata,
            _max_gas_limit: conf.max_gas_limit(),
            interval: conf.interval(),
            deadline: conf.deadline(),
            max_payload_tasks: conf.max_payload_tasks(),
        };

        let payload_generator = PayloadJobGenerator::with_builder(
            ctx.provider().clone(),
            pool,
            ctx.task_executor().clone(),
            payload_job_config,
            ctx.chain_spec().clone(),
            PayloadBuilder::default(),
        );

        let (payload_service, payload_builder) =
            PayloadBuilderService::new(payload_generator, ctx.provider().canonical_state_stream());

        ctx.task_executor()
            .spawn_critical("mev-builder/payload-builder-service", Box::pin(payload_service));

        Ok(payload_builder)
    }
}