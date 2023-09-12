mod reth_ext;

use anyhow::Result;
use clap::Args;
use mev_rs::Network;
use reth_ext::{launch_reth_with, RethNodeExt};

#[derive(Debug, Args)]
#[clap(about = "🛠️ building blocks since 2023")]
pub struct Command {
    #[clap(env, default_value = "config.toml")]
    config_file: String,
}

impl Command {
    pub async fn execute(&self, network: Network) -> Result<()> {
        let ext = RethNodeExt { config_file: self.config_file.clone(), network, config: None };
        launch_reth_with(ext).await;
        Ok(())
    }
}
