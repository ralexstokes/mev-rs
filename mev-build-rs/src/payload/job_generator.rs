use crate::{
    payload::job::PayloadJob,
    utils::payload_job::{duration_until, PayloadTaskGuard},
};
use reth::{
    api::PayloadBuilderAttributes,
    payload::{self, database::CachedReads, error::PayloadBuilderError},
    primitives::{BlockNumberOrTag, Bytes, ChainSpec, B256},
    providers::{BlockReaderIdExt, BlockSource, CanonStateNotification, StateProviderFactory},
    tasks::TaskSpawner,
    transaction_pool::TransactionPool,
};
use reth_basic_payload_builder::{PayloadBuilder, PayloadConfig, PrecachedState};
use std::{sync::Arc, time::Duration};

#[derive(Debug, Clone)]
pub struct PayloadJobGeneratorConfig {
    pub extradata: Bytes,
    pub _max_gas_limit: u64,
    pub interval: Duration,
    pub deadline: Duration,
    pub max_payload_tasks: usize,
}

/// The generator type that creates new jobs that builds empty blocks.
#[derive(Debug)]
pub struct PayloadJobGenerator<Client, Pool, Tasks, Builder> {
    client: Client,
    pool: Pool,
    executor: Tasks,
    config: PayloadJobGeneratorConfig,
    payload_task_guard: PayloadTaskGuard,
    chain_spec: Arc<ChainSpec>,
    builder: Builder,
    pre_cached: Option<PrecachedState>,
}

impl<Client, Pool, Tasks, Builder> PayloadJobGenerator<Client, Pool, Tasks, Builder> {
    pub fn with_builder(
        client: Client,
        pool: Pool,
        executor: Tasks,
        config: PayloadJobGeneratorConfig,
        chain_spec: Arc<ChainSpec>,
        builder: Builder,
    ) -> Self {
        Self {
            client,
            pool,
            executor,
            payload_task_guard: PayloadTaskGuard::new(config.max_payload_tasks),
            config,
            chain_spec,
            builder,
            pre_cached: None,
        }
    }

    #[inline]
    fn max_job_duration(&self, unix_timestamp: u64) -> Duration {
        let duration_until_timestamp = duration_until(unix_timestamp);

        // safety in case clocks are bad
        let duration_until_timestamp = duration_until_timestamp.min(self.config.deadline * 3);

        self.config.deadline + duration_until_timestamp
    }

    #[inline]
    fn job_deadline(&self, unix_timestamp: u64) -> tokio::time::Instant {
        tokio::time::Instant::now() + self.max_job_duration(unix_timestamp)
    }

    fn maybe_pre_cached(&self, parent: B256) -> Option<CachedReads> {
        self.pre_cached.as_ref().filter(|pc| pc.block == parent).map(|pc| pc.cached.clone())
    }
}

impl<Client, Pool, Tasks, Builder> payload::PayloadJobGenerator
    for PayloadJobGenerator<Client, Pool, Tasks, Builder>
where
    Client: StateProviderFactory + BlockReaderIdExt + Clone + Unpin + 'static,
    Pool: TransactionPool + Unpin + 'static,
    Tasks: TaskSpawner + Clone + Unpin + 'static,
    Builder: PayloadBuilder<Pool, Client> + Unpin + 'static,
    <Builder as PayloadBuilder<Pool, Client>>::Attributes: Unpin + Clone,
    <Builder as PayloadBuilder<Pool, Client>>::BuiltPayload: Unpin + Clone,
{
    type Job = PayloadJob<Client, Pool, Tasks, Builder>;

    fn new_payload_job(
        &self,
        attributes: <Builder as PayloadBuilder<Pool, Client>>::Attributes,
    ) -> Result<Self::Job, PayloadBuilderError> {
        let parent_block = if attributes.parent().is_zero() {
            // use latest block if parent is zero: genesis block
            self.client
                .block_by_number_or_tag(BlockNumberOrTag::Latest)?
                .ok_or_else(|| PayloadBuilderError::MissingParentBlock(attributes.parent()))?
                .seal_slow()
        } else {
            let block = self
                .client
                .find_block_by_hash(attributes.parent(), BlockSource::Any)?
                .ok_or_else(|| PayloadBuilderError::MissingParentBlock(attributes.parent()))?;

            // we already know the hash, so we can seal it
            block.seal(attributes.parent())
        };

        let config = PayloadConfig::new(
            Arc::new(parent_block),
            self.config.extradata.clone(),
            attributes,
            Arc::clone(&self.chain_spec),
        );

        let until = self.job_deadline(config.attributes.timestamp());
        let deadline = Box::pin(tokio::time::sleep_until(until));

        let cached_reads = self.maybe_pre_cached(config.parent_block.hash());

        Ok(PayloadJob {
            config,
            client: self.client.clone(),
            pool: self.pool.clone(),
            executor: self.executor.clone(),
            deadline,
            interval: tokio::time::interval(self.config.interval),
            best_payload: None,
            pending_block: None,
            cached_reads,
            payload_task_guard: self.payload_task_guard.clone(),
            builder: self.builder.clone(),
        })
    }

    fn on_new_state(&mut self, new_state: CanonStateNotification) {
        let mut cached = CachedReads::default();

        // extract the state from the notification and put it into the cache
        let committed = new_state.committed();
        let new_state = committed.state();
        for (addr, acc) in new_state.bundle_accounts_iter() {
            if let Some(info) = acc.info.clone() {
                // we want pre cache existing accounts and their storage
                // this only includes changed accounts and storage but is better than nothing
                let storage =
                    acc.storage.iter().map(|(key, slot)| (*key, slot.present_value)).collect();
                cached.insert_account(addr, info, storage);
            }
        }

        self.pre_cached = Some(PrecachedState { block: committed.tip().hash(), cached });
    }
}
