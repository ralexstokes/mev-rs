use beacon_api_client::{mainnet::Client, BlockId};
use mev_rs::{types::AuctionRequest, BlindedBlockRelayer, Relay, RelayEndpoint};
use url::Url;

#[tokio::main]
async fn main() {
    let endpoint = Url::parse("http://localhost:5052").unwrap();
    let beacon_node = Client::new(endpoint);
    let id = BlockId::Head;
    let signed_block = beacon_node.get_beacon_block(id).await.unwrap();
    let slot = signed_block.message().slot() + 1;
    let parent_hash =
        signed_block.message().body().execution_payload().unwrap().block_hash().clone();

    let url = Url::parse("https://0x845bd072b7cd566f02faeb0a4033ce9399e42839ced64e8b2adcfc859ed1e8e1a5a293336a49feac6d9a5edb779be53a@boost-relay-sepolia.flashbots.net/").unwrap();
    let relay_endpoint = RelayEndpoint::try_from(url).unwrap();
    let relay = Relay::try_from(relay_endpoint).unwrap();
    let schedules = relay.get_proposal_schedule().await.unwrap();
    for schedule in schedules {
        if schedule.slot == slot {
            let public_key = schedule.entry.message.public_key;
            let auction_request =
                AuctionRequest { slot, parent_hash: parent_hash.clone(), public_key };
            let signed_bid = relay.fetch_best_bid(&auction_request).await.unwrap();
            let signed_bid_str = serde_json::to_string_pretty(&signed_bid).unwrap();
            println!("{signed_bid_str}");
        }
    }
}
