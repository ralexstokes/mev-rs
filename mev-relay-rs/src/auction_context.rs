use ethereum_consensus::primitives::{BlsPublicKey, U256};
use mev_rs::types::{
    auction_contents, AuctionContents, BlobsBundle, ExecutionPayload, SignedBidSubmission,
    SignedBuilderBid,
};

pub mod bellatrix {
    use super::*;

    #[derive(Debug)]
    pub struct AuctionContext {
        pub builder_public_key: BlsPublicKey,
        pub signed_builder_bid: SignedBuilderBid,
        pub execution_payload: ExecutionPayload,
        pub value: U256,
    }
}

pub mod capella {
    pub use super::bellatrix::*;
}

pub mod deneb {
    use super::*;

    #[derive(Debug)]
    pub struct AuctionContext {
        pub builder_public_key: BlsPublicKey,
        pub signed_builder_bid: SignedBuilderBid,
        pub execution_payload: ExecutionPayload,
        pub value: U256,
        pub blobs_bundle: BlobsBundle,
    }
}

#[derive(Debug)]
pub enum AuctionContext {
    Bellatrix(bellatrix::AuctionContext),
    Capella(capella::AuctionContext),
    Deneb(deneb::AuctionContext),
}

impl AuctionContext {
    // TODO: enforce that `signed_builder_bid` and `signed_submission` have the same fork variant.
    // one way to enforce is to construct and sign the bid here in the constructor.
    pub fn new(
        signed_builder_bid: SignedBuilderBid,
        signed_submission: SignedBidSubmission,
    ) -> Self {
        let builder_public_key = signed_submission.message().builder_public_key.clone();
        let execution_payload = signed_submission.payload().clone();
        let value = signed_submission.message().value;
        match signed_submission {
            SignedBidSubmission::Bellatrix(_) => Self::Bellatrix(bellatrix::AuctionContext {
                builder_public_key,
                signed_builder_bid,
                execution_payload,
                value,
            }),
            SignedBidSubmission::Capella(_) => Self::Capella(bellatrix::AuctionContext {
                builder_public_key,
                signed_builder_bid,
                execution_payload,
                value,
            }),
            SignedBidSubmission::Deneb(submission) => Self::Deneb(deneb::AuctionContext {
                builder_public_key,
                signed_builder_bid,
                execution_payload,
                value,
                blobs_bundle: submission.blobs_bundle,
            }),
        }
    }

    pub fn builder_public_key(&self) -> &BlsPublicKey {
        match self {
            Self::Bellatrix(context) => &context.builder_public_key,
            Self::Capella(context) => &context.builder_public_key,
            Self::Deneb(context) => &context.builder_public_key,
        }
    }

    pub fn signed_builder_bid(&self) -> &SignedBuilderBid {
        match self {
            Self::Bellatrix(context) => &context.signed_builder_bid,
            Self::Capella(context) => &context.signed_builder_bid,
            Self::Deneb(context) => &context.signed_builder_bid,
        }
    }

    pub fn execution_payload(&self) -> &ExecutionPayload {
        match self {
            Self::Bellatrix(context) => &context.execution_payload,
            Self::Capella(context) => &context.execution_payload,
            Self::Deneb(context) => &context.execution_payload,
        }
    }

    pub fn value(&self) -> U256 {
        match self {
            Self::Bellatrix(context) => context.value,
            Self::Capella(context) => context.value,
            Self::Deneb(context) => context.value,
        }
    }

    pub fn to_auction_contents(&self) -> AuctionContents {
        match self {
            Self::Bellatrix(context) => {
                AuctionContents::Bellatrix(context.execution_payload.clone())
            }
            Self::Capella(context) => AuctionContents::Capella(context.execution_payload.clone()),
            Self::Deneb(context) => {
                AuctionContents::Deneb(auction_contents::deneb::AuctionContents {
                    execution_payload: context.execution_payload.clone(),
                    blobs_bundle: context.blobs_bundle.clone(),
                })
            }
        }
    }
}
