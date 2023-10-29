use mev_build_rs::reth_builder::ServiceExt;
use reth::cli::Cli;

pub type Command = Cli<ServiceExt>;
