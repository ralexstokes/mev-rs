pub mod blinded_block_provider;
#[cfg(feature = "serde")]
pub mod serde;
pub mod signing;
pub mod types;

pub use blinded_block_provider::{BlindedBlockProvider, Error as BlindedBlockProviderError};
#[cfg(feature = "api")]
pub use blinded_block_provider::{
    Client as BlindedBlockProviderClient, Server as BlindedBlockProviderServer,
};

pub use signing::{
    sign_builder_message, verify_signed_builder_message, verify_signed_consensus_message,
};
pub use types::*;

use ethereum_consensus::{
    clock::{self, SystemTimeProvider},
    state_transition::Context,
};

#[derive(Default, Debug, Clone, Copy)]
pub enum Network {
    #[default]
    Mainnet,
    Sepolia,
    Goerli,
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let repr = match self {
            Self::Mainnet => "mainnet",
            Self::Sepolia => "sepolia",
            Self::Goerli => "goerli",
        };
        write!(f, "{repr}")
    }
}

impl From<Network> for Context {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => Context::for_mainnet(),
            Network::Sepolia => Context::for_sepolia(),
            Network::Goerli => Context::for_goerli(),
        }
    }
}

impl From<Network> for clock::Clock<SystemTimeProvider> {
    fn from(network: Network) -> Self {
        match network {
            Network::Mainnet => clock::for_mainnet(),
            Network::Sepolia => clock::for_sepolia(),
            Network::Goerli => clock::for_goerli(),
        }
    }
}
