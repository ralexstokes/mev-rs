use reth::cli::{
        config::RethRpcConfig,
        ext::{RethCliExt, RethNodeCommandConfig},
        components::{RethNodeComponents, RethRpcComponents},
    };

use crate::rpc::ValidationApiServer;
use crate::ValidationApi;

pub struct ValidationCliExt;

impl RethCliExt for ValidationCliExt {
    type Node = RethCliValidationApi;
}

#[derive(Debug, Clone, Copy, Default, clap::Args)]
pub struct RethCliValidationApi {
    #[clap(long)]
    pub enable_ext: bool,
}

impl RethNodeCommandConfig for RethCliValidationApi {
    fn extend_rpc_modules<Conf, Reth>(
        &mut self,
        _config: &Conf,
        _components: &Reth,
        rpc_components: RethRpcComponents<'_, Reth>,
    ) -> eyre::Result<()>
    where
        Conf: RethRpcConfig,
        Reth: RethNodeComponents,
    {
        if !self.enable_ext {
            return Ok(());
        }

        let provider = rpc_components.registry.provider().clone();
        let ext = ValidationApi::new(provider);

        rpc_components.modules.merge_configured(ext.into_rpc())?;

        println!("validation extension enabled");
        Ok(())
    }
}
