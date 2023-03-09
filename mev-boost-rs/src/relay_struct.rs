use ethereum_consensus::primitives::BlsPublicKey;
use mev_rs::blinded_block_provider::Client as BlindedBlockProviderClient;
use url::Url;

pub struct RelayStruct {
    pub api: BlindedBlockProviderClient,
    public_key: BlsPublicKey,
    pub endpoint: Url,
}

impl RelayStruct {
    pub fn new(api: BlindedBlockProviderClient, public_key: BlsPublicKey, endpoint: Url) -> Self {
        Self { api, public_key, endpoint }
    }
}
