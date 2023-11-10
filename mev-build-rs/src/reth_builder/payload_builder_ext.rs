use crate::reth_builder::build::BuildContext;
use reth_basic_payload_builder::{
    default_payload_builder, BuildArguments, BuildOutcome, Cancelled, PayloadConfig,
};
use reth_payload_builder::error::PayloadBuilderError;
use reth_primitives::U256;

pub struct RethPayloadBuilder<Pool, Client> {
    build_arguments: BuildArguments<Pool, Client>,
}

impl<Pool, Client> RethPayloadBuilder<Pool, Client>
where
    Client: reth_provider::StateProviderFactory,
    Pool: reth_transaction_pool::TransactionPool,
{
    pub fn new(
        context: &BuildContext,
        threshold_value: U256,
        client: Client,
        pool: Pool,
        cancel: Cancelled,
    ) -> Self {
        let cached_reads = Default::default();
        let config = PayloadConfig::new(
            context.parent_block.clone(),
            context.extra_data.clone(),
            context.payload_attributes.clone(),
            context.chain_spec.clone(),
        );

        let build_arguments = BuildArguments::new(client, pool, cached_reads, config, cancel, None);

        Self { build_arguments }
    }

    pub fn build(self) -> Result<BuildOutcome, PayloadBuilderError> {
        let result = default_payload_builder(self.build_arguments)?;
        match result {
            BuildOutcome::Aborted { fees, cached_reads } => {
                //
            }
            BuildOutcome::Better { payload, cached_reads } => {
                //
            }
            BuildOutcome::Cancelled => {
                //
            }
        }
    }
}
