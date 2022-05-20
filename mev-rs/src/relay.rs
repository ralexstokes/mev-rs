use anyhow::Result;
use clap::{Args, Subcommand};

#[derive(Debug, Args)]
#[clap(
    about = "connecting builders to proposers",
    subcommand_negates_reqs = true
)]
pub(crate) struct Command {
    #[clap(required = true)]
    config_file: Option<String>,

    #[clap(subcommand)]
    command: Option<Commands>,
}

impl Command {
    pub(crate) async fn execute(&self) -> Result<()> {
        if let Some(subcommand) = self.command.as_ref() {
            subcommand.execute()
        } else {
            run_relay_from(self.config_file.as_ref().unwrap())
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    Mock { config_file: String },
}

impl Commands {
    fn execute(&self) -> Result<()> {
        match self {
            Self::Mock { config_file } => run_mock_from(config_file),
        }
    }
}

fn run_relay_from(config_file: &str) -> Result<()> {
    println!("running relay from {}", config_file);
    Ok(())
}

fn run_mock_from(config_file: &str) -> Result<()> {
    println!("running mock relay from {}", config_file);
    Ok(())
}
