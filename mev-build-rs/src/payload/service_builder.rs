use crate::{
    node::BuilderEngineTypes,
    payload::{
        builder::PayloadBuilder,
        job_generator::{PayloadJobGenerator, PayloadJobGeneratorConfig},
    },
    service::BuilderConfig as Config,
    Error,
};
use alloy_signer_local::{coins_bip39::English, MnemonicBuilder, PrivateKeySigner};
use reth::{
    builder::{node::FullNodeTypes, BuilderContext, NodeTypesWithEngine},
    chainspec::ChainSpec,
    cli::config::PayloadBuilderConfig,
    payload::{EthBuiltPayload, PayloadBuilderHandle, PayloadBuilderService},
    primitives::revm_primitives::Bytes,
    providers::CanonStateSubscriptions,
    transaction_pool::TransactionPool,
};
use tokio::sync::mpsc::Sender;

fn signer_from_mnemonic(mnemonic: &str) -> Result<PrivateKeySigner, Error> {
    MnemonicBuilder::<English>::default().phrase(mnemonic).build().map_err(Into::into)
}

#[derive(Debug, Clone)]
pub struct PayloadServiceBuilder {
    extra_data: Option<Bytes>,
    signer: PrivateKeySigner,
    bid_tx: Sender<EthBuiltPayload>,
}

impl TryFrom<(&Config, Sender<EthBuiltPayload>)> for PayloadServiceBuilder {
    type Error = Error;

    fn try_from((value, bid_tx): (&Config, Sender<EthBuiltPayload>)) -> Result<Self, Self::Error> {
        let signer = signer_from_mnemonic(&value.execution_mnemonic)?;
        Ok(Self { extra_data: value.extra_data.clone(), signer, bid_tx })
    }
}

impl<Node, Pool> reth::builder::components::PayloadServiceBuilder<Node, Pool>
    for PayloadServiceBuilder
where
    Node: FullNodeTypes<
        Types: NodeTypesWithEngine<Engine = BuilderEngineTypes, ChainSpec = ChainSpec>,
    >,
    Pool: TransactionPool + Unpin + 'static,
{
    async fn spawn_payload_service(
        self,
        ctx: &BuilderContext<Node>,
        pool: Pool,
    ) -> eyre::Result<PayloadBuilderHandle<<Node::Types as NodeTypesWithEngine>::Engine>> {
        let chain_id = ctx.chain_spec().chain().id();
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
            PayloadBuilder::new(self.bid_tx, self.signer, chain_id, ctx.chain_spec().clone()),
        );

        let (payload_service, payload_builder) =
            PayloadBuilderService::new(payload_generator, ctx.provider().canonical_state_stream());

        ctx.task_executor()
            .spawn_critical("mev-builder/payload-builder-service", Box::pin(payload_service));

        Ok(payload_builder)
    }
}
