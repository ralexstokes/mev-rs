use ethereum_consensus::state_transition::{Context, Error};

#[derive(Default, Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
pub enum Network {
    #[default]
    Mainnet,
    Sepolia,
    Goerli,
    Custom(String),
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mainnet => write!(f, "mainnet"),
            Self::Sepolia => write!(f, "sepolia"),
            Self::Goerli => write!(f, "goerli"),
            Self::Custom(config) => write!(f, "custom network with config at `{config}`"),
        }
    }
}

impl TryFrom<&Network> for Context {
    type Error = Error;

    fn try_from(network: &Network) -> Result<Self, Self::Error> {
        match network {
            Network::Mainnet => Ok(Context::for_mainnet()),
            Network::Sepolia => Ok(Context::for_sepolia()),
            Network::Goerli => Ok(Context::for_goerli()),
            Network::Custom(config) => Context::try_from_file(config),
        }
    }
}
