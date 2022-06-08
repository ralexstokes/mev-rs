use ethereum_consensus::primitives::{ExecutionAddress, Hash32};
use tokio::sync::mpsc;
use url::Url;

pub struct BuildJob {
    pub head_block_hash: Hash32,
    pub timestamp: u64,
    pub suggested_fee_recipient: ExecutionAddress,
    pub payload_id: u64,
}

pub struct EngineProxy {
    proxy_endpoint: Url,
    engine_api_endpoint: Url,
    build_jobs: mpsc::Sender<BuildJob>,
}

impl EngineProxy {
    pub fn new(
        proxy_endpoint: Url,
        engine_api_endpoint: Url,
        build_jobs: mpsc::Sender<BuildJob>,
    ) -> Self {
        Self {
            proxy_endpoint,
            engine_api_endpoint,
            build_jobs,
        }
    }

    pub async fn run(&mut self) {
        // host JSON_RPC proxy server at `proxy_endpoint`

        // watch for methods, if `engine_forkchoiceUpdatedV1`
        // -- data to grab
        //    -- headBlockHash     -> parentHash
        //    -- timestamp         -> slot
        //    -- suggestedFeeRecip -> reverse lookup for pubkey
        // => have `PayloadRequest` for this transaction
        // send upstream
        // then: grab `payloadId` from response body
        // assemble `BuildJob` and send on sender channel
    }
}
