use crate::{
    node::BuilderEngineTypes,
    payload::{
        builder::PayloadBuilder,
        job_generator::{PayloadJobGenerator, PayloadJobGeneratorConfig},
    },
    service::BuilderConfig as Config,
    Error,
};
use alloy_signer_wallet::{coins_bip39::English, LocalWallet, MnemonicBuilder};
use reth::{
    builder::{node::FullNodeTypes, BuilderContext},
    cli::config::PayloadBuilderConfig,
    payload::{PayloadBuilderHandle, PayloadBuilderService},
    primitives::{Address, Bytes},
    providers::CanonStateSubscriptions,
    transaction_pool::TransactionPool,
};

fn signer_from_mnemonic(mnemonic: &str) -> Result<LocalWallet, Error> {
    MnemonicBuilder::<English>::default().phrase(mnemonic).build().map_err(Into::into)
}

#[derive(Debug, Clone)]
pub struct PayloadServiceBuilder {
    extra_data: Option<Bytes>,
    signer: LocalWallet,
    fee_recipient: Address,
}

impl TryFrom<&Config> for PayloadServiceBuilder {
    type Error = Error;

    fn try_from(value: &Config) -> Result<Self, Self::Error> {
        let signer = signer_from_mnemonic(&value.execution_mnemonic)?;
        let fee_recipient = value.fee_recipient.unwrap_or_else(|| signer.address());
        Ok(Self { extra_data: value.extra_data.clone(), signer, fee_recipient })
    }
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
            ctx.chain_spec().clone(),
            PayloadBuilder::new(self.signer, self.fee_recipient, chain_id),
        );

        let (payload_service, payload_builder) =
            PayloadBuilderService::new(payload_generator, ctx.provider().canonical_state_stream());

        ctx.task_executor()
            .spawn_critical("mev-builder/payload-builder-service", Box::pin(payload_service));

        Ok(payload_builder)
    }
}
