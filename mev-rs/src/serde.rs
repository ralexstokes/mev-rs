pub(crate) use ethereum_consensus::serde::as_str;

#[cfg(test)]
mod tests {
    use ethereum_consensus::{primitives::U256, types::mainnet::ExecutionPayloadHeader};

    use crate::types::{builder_bid::deneb, AuctionRequest, BuilderBid, SignedBuilderBid};

    #[test]
    fn test_fmt() {
        let signed_bid = SignedBuilderBid {
            message: BuilderBid::Deneb(deneb::BuilderBid {
                header: ExecutionPayloadHeader::Deneb(Default::default()),
                blob_kzg_commitments: Default::default(),
                value: U256::from(234),
                public_key: Default::default(),
            }),
            signature: Default::default(),
        };

        dbg!(&signed_bid);
        println!("{signed_bid}");
        let auction_request = AuctionRequest::default();
        dbg!(&auction_request);
        println!("{auction_request}");
    }
}
