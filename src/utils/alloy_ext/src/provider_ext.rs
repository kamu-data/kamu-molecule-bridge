use alloy::eips::BlockNumberOrTag;
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::{Filter, Log};
use async_trait::async_trait;
use color_eyre::eyre;
use color_eyre::eyre::ContextCompat;

pub struct LogsChunk {
    pub from_block: u64,
    pub to_block: u64,
    pub logs: Vec<Log>,
}

#[async_trait]
pub trait ProviderExt {
    async fn get_logs_ext<F>(&self, filter: &Filter, callback: &mut F) -> eyre::Result<()>
    where
        F: FnMut(LogsChunk) -> eyre::Result<()> + Send + Sync;

    async fn latest_finalized_block_number(&self) -> eyre::Result<u64>;
}

#[async_trait]
impl ProviderExt for DynProvider {
    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(
            from = filter.get_from_block().unwrap_or_default(),
            to = filter.get_to_block().unwrap_or_default(),
            diff = filter.get_to_block().unwrap_or_default() - filter.get_from_block().unwrap_or_default(),
        )
    )]
    async fn get_logs_ext<F>(&self, filter: &Filter, callback: &mut F) -> eyre::Result<()>
    where
        F: FnMut(LogsChunk) -> eyre::Result<()> + Send + Sync,
    {
        // TODO: retry logic
        // TODO: Handle RPC errors (too many events)
        let logs = self.get_logs(filter).await?;

        (*callback)(LogsChunk {
            from_block: filter.get_from_block().unwrap_or_default(),
            to_block: filter.get_from_block().unwrap_or_default(),
            logs,
        })?;

        Ok(())
    }

    async fn latest_finalized_block_number(&self) -> eyre::Result<u64> {
        // TODO: retry logic
        let block = self
            .get_block_by_number(BlockNumberOrTag::Finalized)
            .await?
            .context("Latest finalized block is missed")?;

        Ok(block.header.number)
    }
}
