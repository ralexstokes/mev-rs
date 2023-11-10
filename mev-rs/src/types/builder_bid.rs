use crate::{
    signing::{compute_builder_signing_root, sign_builder_message, verify_signature, SecretKey},
    types::ExecutionPayloadHeader,
};
use ethereum_consensus::{
    deneb::{mainnet::MAX_BLOB_COMMITMENTS_PER_BLOCK, polynomial_commitments::KzgCommitment},
    primitives::{BlsPublicKey, BlsSignature},
    ssz::prelude::*,
    state_transition::Context,
    Error, Fork,
};
use std::fmt;

pub mod bellatrix {
    use super::ExecutionPayloadHeader;
    use ethereum_consensus::{
        primitives::{BlsPublicKey, U256},
        ssz::prelude::*,
    };
    #[derive(Debug, Clone, SimpleSerialize, PartialEq, Eq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct BuilderBid {
        pub header: ExecutionPayloadHeader,
        #[serde(with = "crate::serde::as_str")]
        pub value: U256,
        #[serde(rename = "pubkey")]
        pub public_key: BlsPublicKey,
    }
}

pub mod capella {
    pub use super::bellatrix::*;
}

pub mod deneb {
    use super::{KzgCommitment, MAX_BLOB_COMMITMENTS_PER_BLOCK};
    use crate::types::ExecutionPayloadHeader;
    use ethereum_consensus::{
        primitives::{BlsPublicKey, U256},
        ssz::prelude::*,
    };
    #[derive(Debug, Clone, SimpleSerialize, PartialEq, Eq)]
    #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
    pub struct BuilderBid {
        pub header: ExecutionPayloadHeader,
        pub blob_kzg_commitments: List<KzgCommitment, MAX_BLOB_COMMITMENTS_PER_BLOCK>,
        #[serde(with = "crate::serde::as_str")]
        pub value: U256,
        #[serde(rename = "pubkey")]
        pub public_key: BlsPublicKey,
    }
}

#[derive(Debug, Clone, SimpleSerialize, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[serde(untagged)]
#[ssz(transparent)]
pub enum BuilderBid {
    Bellatrix(bellatrix::BuilderBid),
    Capella(capella::BuilderBid),
    Deneb(deneb::BuilderBid),
}

impl<'de> serde::Deserialize<'de> for BuilderBid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        if let Ok(inner) = <_ as serde::Deserialize>::deserialize(&value) {
            return Ok(Self::Deneb(inner))
        }
        if let Ok(inner) = <_ as serde::Deserialize>::deserialize(&value) {
            return Ok(Self::Capella(inner))
        }
        if let Ok(inner) = <_ as serde::Deserialize>::deserialize(&value) {
            return Ok(Self::Bellatrix(inner))
        }
        Err(serde::de::Error::custom("no variant could be deserialized from input"))
    }
}

impl BuilderBid {
    pub fn version(&self) -> Fork {
        match self {
            Self::Bellatrix(..) => Fork::Bellatrix,
            Self::Capella(..) => Fork::Capella,
            Self::Deneb(..) => Fork::Deneb,
        }
    }

    pub fn header(&self) -> &ExecutionPayloadHeader {
        match self {
            Self::Bellatrix(inner) => &inner.header,
            Self::Capella(inner) => &inner.header,
            Self::Deneb(inner) => &inner.header,
        }
    }

    pub fn blob_kzg_commitments(
        &self,
    ) -> Option<&List<KzgCommitment, MAX_BLOB_COMMITMENTS_PER_BLOCK>> {
        match self {
            Self::Deneb(inner) => Some(&inner.blob_kzg_commitments),
            _ => None,
        }
    }

    pub fn value(&self) -> U256 {
        match self {
            Self::Bellatrix(inner) => inner.value,
            Self::Capella(inner) => inner.value,
            Self::Deneb(inner) => inner.value,
        }
    }

    pub fn public_key(&self) -> &BlsPublicKey {
        match self {
            Self::Bellatrix(inner) => &inner.public_key,
            Self::Capella(inner) => &inner.public_key,
            Self::Deneb(inner) => &inner.public_key,
        }
    }

    pub fn sign(
        mut self,
        secret_key: &SecretKey,
        context: &Context,
    ) -> Result<SignedBuilderBid, Error> {
        let signature = sign_builder_message(&mut self, secret_key, context)?;
        Ok(SignedBuilderBid { message: self, signature })
    }
}

#[derive(Debug, Clone, SimpleSerialize, serde::Serialize, serde::Deserialize)]
pub struct SignedBuilderBid {
    pub message: BuilderBid,
    pub signature: BlsSignature,
}

impl SignedBuilderBid {
    pub fn version(&self) -> Fork {
        self.message.version()
    }
}

impl fmt::Display for SignedBuilderBid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let block_hash = self.message.header().block_hash();
        let value = self.message.value();
        write!(f, "block hash {block_hash} and value {value}")
    }
}

impl SignedBuilderBid {
    pub fn verify_signature(&mut self, context: &Context) -> Result<(), Error> {
        let signing_root = compute_builder_signing_root(&mut self.message, context)?;
        let public_key = self.message.public_key();
        verify_signature(public_key, signing_root.as_ref(), &self.signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signing::sign_builder_message;
    use ethereum_consensus::{crypto::SecretKey, state_transition::Context};
    use rand::prelude::*;

    const SIGNED_BUILDER_BID_JSON: &str = r#"
    {
        "message": {
          "header": {
            "parent_hash": "0xac6e636151a5c90dd7179b5ca62c1e759dd75505ba95d060b9ea2a8e342f88f4",
            "fee_recipient": "0x1e2cd78882b12d3954a049fd82ffd691565dc0a5",
            "state_root": "0x223c37043a5c9ab328fef7d2a58bc01da7f994252eb552343b97faf6e647d633",
            "receipts_root": "0x8b0f90f0a7ad8e3135f9b95d9234c84a4a0440ab8df18327bab6bbc6a5d9efe5",
            "logs_bloom": "0x40000008081200020008100002000000042040100420000000000000000000000004000800240000040001000008400010820400004000000801210800648120000100504002042000000008080180020000100000244200080000000004002808100040020008000281008000000810100500000010000000000010000000000080000000a0000020020080404060000001001800800810081000028c50002102084000080000020000030000040018100060000040000000401010240000000400000a0000101000020000008060002008100000a0000002008400000060000096000000200010000000000000980010804508000000080000010200200840",
            "prev_randao": "0x5e93d21bf689fd1c293a85dbb93681383867abe057375890a251404bda3417f9",
            "block_number": "4522537",
            "gas_limit": "30000000",
            "gas_used": "4483564",
            "timestamp": "1697757948",
            "extra_data": "0x496c6c756d696e61746520446d6f63726174697a6520447374726962757465",
            "base_fee_per_gas": "9",
            "block_hash": "0xf0029e1f18f5bc8944c9ce4453d93f1772e3ac6626470024c8def699271def2e",
            "transactions_root": "0xbf12054777b89c3a25b78281604fc99d5e55cb9fedafcce4dc688779f65197ee",
            "withdrawals_root": "0xa427d204f34246cdec36b4db9a94f25e08a5be2f7e670ff3072ceb241e8934f6"
          },
          "value": "2591493712581794",
          "pubkey": "0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a"
        },
        "signature": "0xafb17f2861b808f4728bbc31aeaa36e9b86465ff08fc3a4ccfd302403b48dfe8fc12cfe30349d95822142668187882f0000fc1ea5ae30ea0c6f44d8d3a535f1945d10b7954642a52dec65fbe929e6b09b626c19318e88cea99c38b414589c6f1"
      }
    "#;

    #[test]
    fn test_builder_bid_signature() {
        let mut rng = thread_rng();
        let key = SecretKey::random(&mut rng).unwrap();
        let public_key = key.public_key();
        let mut builder_bid = capella::BuilderBid {
            header: ExecutionPayloadHeader::Capella(Default::default()),
            value: U256::from(234234),
            public_key,
        };
        let context = Context::for_holesky();
        let signature = sign_builder_message(&mut builder_bid, &key, &context).unwrap();
        let mut signed_builder_bid =
            SignedBuilderBid { message: BuilderBid::Capella(builder_bid), signature };
        signed_builder_bid.verify_signature(&context).expect("is valid signature");
    }

    #[test]
    fn test_builder_bid_signature_from_relay() {
        let mut signed_builder_bid: SignedBuilderBid =
            serde_json::from_str(SIGNED_BUILDER_BID_JSON.trim()).unwrap();
        let context = Context::for_sepolia();
        signed_builder_bid.verify_signature(&context).expect("is valid signature");
    }
}
