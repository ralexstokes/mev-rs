use clap::Args;

use crate::config::Config;

#[derive(Debug, Args)]
#[clap(about = "🔬 (debug) utility to verify configuration")]
pub struct Command {
    #[clap(env)]
    config_file: String,
}

impl Command {
    pub async fn execute(self) -> eyre::Result<()> {
        let config_file = self.config_file;

        let config = Config::from_toml_file(config_file)?;
        tracing::info!("{config:#?}");

        Ok(())
    }
}
