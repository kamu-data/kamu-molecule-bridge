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
    async fn get_logs_ext<F>(&self, filter: &Filter, callback: F) -> eyre::Result<()>
    where
        F: FnMut(LogsChunk) -> eyre::Result<()> + Send;

    async fn latest_finalized_block_number(&self) -> eyre::Result<u64>;
}

#[async_trait]
impl ProviderExt for DynProvider {
    async fn get_logs_ext<F>(&self, filter: &Filter, mut callback: F) -> eyre::Result<()>
    where
        F: FnMut(LogsChunk) -> eyre::Result<()> + Send,
    {
        // TODO: Handle RPC errors (too many events)
        let logs = self.get_logs(filter).await?;

        callback(LogsChunk {
            from_block: filter.get_from_block().unwrap_or_default(),
            to_block: filter.get_from_block().unwrap_or_default(),
            logs,
        })?;

        Ok(())
    }

    async fn latest_finalized_block_number(&self) -> eyre::Result<u64> {
        let block = self
            .get_block_by_number(BlockNumberOrTag::Finalized)
            .await?
            .context("Latest finalized block is missed")?;

        Ok(block.header.number)
    }
}
