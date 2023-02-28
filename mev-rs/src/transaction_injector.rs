use ethers::{
    prelude::*, signers::coins_bip39::English, types::transaction::eip2718::TypedTransaction, utils,
};
use serde::Deserialize;
use thiserror::Error;
use url::ParseError;

#[derive(Debug, Error)]
pub enum Error {
    #[error("issue constructing wallet: {0}")]
    Wallet(#[from] WalletError),
    #[error("could not parse URL: {0}")]
    Url(#[from] ParseError),
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    mnemonic: String,
    chain_id: u64,
    provider_url: String,
}

type LocalSigner = SignerMiddleware<Provider<Http>, LocalWallet>;

#[derive(Debug)]
pub struct Injector {
    first_signer: LocalSigner,
    second_signer: LocalSigner,
    senders_turn: bool,
}

impl Injector {
    pub fn new(config: Config) -> Result<Self, Error> {
        let Config { mnemonic, chain_id, provider_url } = config;
        let first_signer =
            MnemonicBuilder::<English>::default().phrase(mnemonic.as_str()).index(0u32)?.build()?;
        let second_signer =
            MnemonicBuilder::<English>::default().phrase(mnemonic.as_str()).index(1u32)?.build()?;
        let provider = Provider::<Http>::try_from(provider_url)?;
        let first_signer =
            SignerMiddleware::new(provider.clone(), first_signer.with_chain_id(chain_id));
        let second_signer = SignerMiddleware::new(provider, second_signer.with_chain_id(chain_id));
        Ok(Self { first_signer, second_signer, senders_turn: false })
    }

    // Send some ETH from one signer to the other, alternating signers with each successful call to
    // this function
    pub async fn submit_transaction(&mut self) -> Result<TxHash, Error> {
        let (sender, recipient) = if self.senders_turn {
            (&self.second_signer, &self.first_signer)
        } else {
            (&self.first_signer, &self.second_signer)
        };

        let value = utils::parse_ether(0.05).unwrap();
        let fee = 52_003_004_005u64;

        let msg = "bytes from the builder".as_bytes().to_vec();
        let mut txn = TypedTransaction::Eip1559(
            Eip1559TransactionRequest::new()
                .from(sender.address())
                .to(recipient.address())
                .value(value)
                .data(msg)
                .max_priority_fee_per_gas(fee)
                .max_fee_per_gas(fee),
        );
        sender.fill_transaction(&mut txn, None).await.unwrap();
        let pending_transaction = sender.send_transaction(txn, None).await.unwrap();
        let receipt = pending_transaction.confirmations(1).await.unwrap().unwrap();

        self.senders_turn = !self.senders_turn;

        Ok(receipt.transaction_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[tokio::test]
    async fn build_context() {
        let mnemonic = env::var("MNEMONIC").unwrap_or_else(|_| {
            "work man father plunge mystery proud hollow address reunion sauce theory bonus"
                .to_string()
        });
        let chain_id = 1337803;
        let provider_url = "http://localhost:8545";
        let config = Config { mnemonic, chain_id, provider_url: provider_url.to_string() };
        let mut context = Injector::new(config).unwrap();
        let txn_hash = context.submit_transaction().await.unwrap();
        dbg!(txn_hash);
        let txn_hash = context.submit_transaction().await.unwrap();
        dbg!(txn_hash);
    }
}
