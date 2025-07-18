use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{Address, B256};
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::{Filter, Log};
use alloy::transports::RpcError;
use async_trait::async_trait;
use color_eyre::eyre;
use color_eyre::eyre::{ContextCompat, bail};
use std::collections::HashSet;
use std::time::Duration;

pub struct LogsChunk {
    pub from_block: u64,
    pub to_block: u64,
    pub logs: Vec<Log>,
}

#[async_trait]
pub trait ProviderExt {
    async fn get_logs_ext<F>(
        &self,
        addresses: Vec<Address>,
        hash_set: HashSet<B256>,
        from_block: u64,
        to_block: u64,
        callback: &mut F,
    ) -> eyre::Result<()>
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
            addresses_count = addresses.len(),
            event_signatures_count = event_signatures.len(),
            from = from_block,
            to = to_block,
            diff = to_block - from_block,
        )
    )]
    async fn get_logs_ext<F>(
        &self,
        addresses: Vec<Address>,
        event_signatures: HashSet<B256>,
        from_block: u64,
        to_block: u64,
        callback: &mut F,
    ) -> eyre::Result<()>
    where
        F: FnMut(LogsChunk) -> eyre::Result<()> + Send + Sync,
    {
        const MAX_ADDRESSES_PER_RPC_REQUEST: usize = 25;

        for address_window in addresses.chunks(MAX_ADDRESSES_PER_RPC_REQUEST) {
            get_logs_ext_internal(
                self,
                address_window.to_vec(),
                event_signatures.clone(),
                from_block,
                to_block,
                callback,
                0,
            )
            .await?;
        }

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

#[tracing::instrument(
    level = "debug",
    skip_all,
    fields(
        addresses_count = addresses.len(),
        event_signatures_count = event_signatures.len(),
        from = from_block,
        to = to_block,
        diff = to_block - from_block,
    )
)]
async fn get_logs_ext_internal<F>(
    provider: &DynProvider,
    addresses: Vec<Address>,
    event_signatures: HashSet<B256>,
    from_block: u64,
    to_block: u64,
    callback: &mut F,
    retry_count: usize,
) -> eyre::Result<()>
where
    F: FnMut(LogsChunk) -> eyre::Result<()> + Send + Sync,
{
    debug_assert!(to_block >= from_block);
    debug_assert!(!addresses.is_empty());
    debug_assert!(!event_signatures.is_empty());

    const MAX_RETRY_COUNT: usize = 3;
    const DELAY_BETWEEN_RETRIES_STEP: Duration = Duration::from_secs(1);

    if retry_count >= MAX_RETRY_COUNT {
        bail!("Too many retries")
    }

    let filter = Filter::new()
        .address(addresses.clone())
        .event_signature(event_signatures.clone())
        .from_block(from_block)
        .to_block(to_block);

    // TODO: generalize retry logic
    // TODO: Handle RPC errors (too many events)
    let logs = match provider.get_logs(&filter).await {
        Ok(logs) => logs,
        Err(RpcError::Transport(e)) if e.is_retry_err() => {
            let retry_delay = DELAY_BETWEEN_RETRIES_STEP * (retry_count + 1) as u32;
            tokio::time::sleep(retry_delay).await;

            return Box::pin(get_logs_ext_internal(
                provider,
                addresses,
                event_signatures,
                from_block,
                to_block,
                callback,
                retry_count + 1,
            ))
            .await;
        }
        unexpected_error @ Err(_) => unexpected_error?,
    };

    (*callback)(LogsChunk {
        from_block: filter.get_from_block().unwrap_or_default(),
        to_block: filter.get_from_block().unwrap_or_default(),
        logs,
    })?;

    Ok(())
}
