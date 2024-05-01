use ethereum_consensus::{
    primitives::{BlsPublicKey, U256},
    state_transition::Context,
};
use mev_rs::{
    signing::{sign_builder_message, SecretKey},
    types::{
        auction_contents, builder_bid, AuctionContents, BlobsBundle, BuilderBid, ExecutionPayload,
        ExecutionPayloadHeader, SignedBidSubmission, SignedBuilderBid,
    },
    Error,
};

fn to_header(execution_payload: &ExecutionPayload) -> Result<ExecutionPayloadHeader, Error> {
    let header = match execution_payload {
        ExecutionPayload::Bellatrix(payload) => {
            ExecutionPayloadHeader::Bellatrix(payload.try_into()?)
        }
        ExecutionPayload::Capella(payload) => ExecutionPayloadHeader::Capella(payload.try_into()?),
        ExecutionPayload::Deneb(payload) => ExecutionPayloadHeader::Deneb(payload.try_into()?),
    };
    Ok(header)
}

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
    pub fn new(
        signed_submission: SignedBidSubmission,
        relay_public_key: BlsPublicKey,
        relay_secret_key: &SecretKey,
        context: &Context,
    ) -> Result<Self, Error> {
        let builder_public_key = signed_submission.message().builder_public_key.clone();

        let execution_payload = signed_submission.payload().clone();
        let execution_payload_header = to_header(&execution_payload)?;

        let value = signed_submission.message().value;

        let bid = match signed_submission {
            SignedBidSubmission::Bellatrix(_) => {
                BuilderBid::Bellatrix(builder_bid::bellatrix::BuilderBid {
                    header: execution_payload_header,
                    value,
                    public_key: relay_public_key,
                })
            }
            SignedBidSubmission::Capella(_) => {
                BuilderBid::Capella(builder_bid::capella::BuilderBid {
                    header: execution_payload_header,
                    value,
                    public_key: relay_public_key,
                })
            }
            SignedBidSubmission::Deneb(ref submission) => {
                BuilderBid::Deneb(builder_bid::deneb::BuilderBid {
                    header: execution_payload_header,
                    blob_kzg_commitments: submission.blobs_bundle.commitments.clone(),
                    value,
                    public_key: relay_public_key,
                })
            }
        };

        let signature = sign_builder_message(&bid, relay_secret_key, context)?;
        let signed_builder_bid = SignedBuilderBid { message: bid, signature };

        let auction_context = match signed_submission {
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
        };

        Ok(auction_context)
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
