use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{Address, B256};
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::{Filter, Log};
use alloy::transports::{RpcError, TransportErrorKind};
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
        event_signatures: HashSet<B256>,
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
            diff = to_block.checked_sub(from_block),
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
            binary_search_logs(
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
        diff = to_block.checked_sub(from_block),
        retry_count = retry_count,
    )
)]
async fn binary_search_logs<F>(
    provider: &DynProvider,
    addresses: Vec<Address>,
    event_signatures: HashSet<B256>,
    from_block: u64,
    to_block: u64,
    callback: &mut F,
    retry_count: u32,
) -> eyre::Result<()>
where
    F: FnMut(LogsChunk) -> eyre::Result<()> + Send + Sync,
{
    debug_assert!(to_block >= from_block, "{to_block} >= {from_block}");
    debug_assert!(!addresses.is_empty());
    debug_assert!(!event_signatures.is_empty());

    const MAX_RETRY_COUNT: u32 = 3;
    const DELAY_BETWEEN_RETRIES_STEP: Duration = Duration::from_secs(1);
    const MIN_BLOCK_RANGE: u64 = 1;

    if retry_count >= MAX_RETRY_COUNT {
        bail!("Too many retries for block range [{from_block}, {to_block}]");
    }

    let filter = Filter::new()
        .address(addresses.clone())
        .event_signature(event_signatures.clone())
        .from_block(from_block)
        .to_block(to_block);

    match provider.get_logs(&filter).await {
        Ok(logs) => {
            callback(LogsChunk {
                from_block,
                to_block,
                logs,
            })?;

            Ok(())
        }
        Err(e) if is_too_many_events_error(&e) => {
            let current_range = to_block - from_block + 1;

            if current_range <= MIN_BLOCK_RANGE {
                bail!("Cannot split block range [{from_block}, {to_block}] further: {e}",);
            }

            tracing::warn!(
                "Too many events error for range [{from_block}, {to_block}], splitting in half",
            );

            // Binary search: split the range in half
            let mid_block = from_block + (to_block - from_block) / 2;

            // Process first half
            Box::pin(binary_search_logs(
                provider,
                addresses.clone(),
                event_signatures.clone(),
                from_block,
                mid_block,
                callback,
                0, // Reset retry count for new range
            ))
            .await?;

            // Process second half
            Box::pin(binary_search_logs(
                provider,
                addresses,
                event_signatures,
                mid_block + 1,
                to_block,
                callback,
                0, // Reset retry count for new range
            ))
            .await?;

            Ok(())
        }
        Err(RpcError::Transport(e)) if e.is_retry_err() => {
            // Network error, retry with exponential backoff
            let retry_delay = DELAY_BETWEEN_RETRIES_STEP * (retry_count + 1);

            tracing::debug!(
                "Retryable error, waiting {retry_delay:?} before retry #{} for range [{from_block}, {to_block}]",
                retry_count + 1,
            );

            tokio::time::sleep(retry_delay).await;

            Box::pin(binary_search_logs(
                provider,
                addresses,
                event_signatures,
                from_block,
                to_block,
                callback,
                retry_count + 1,
            ))
            .await
        }
        Err(e) => Err(e.into()),
    }
}

fn is_too_many_events_error(error: &RpcError<TransportErrorKind>) -> bool {
    let error = match error {
        RpcError::ErrorResp(resp) => resp.message.to_lowercase(),
        RpcError::Transport(e) => e.to_string().to_lowercase(),
        _ => return false,
    };

    #[expect(clippy::match_same_arms)]
    match error.as_str() {
        // Alchemy
        "log response size exceeded" | "query timeout" => true,
        // Infura
        "query returned more than" | "request timed out" => true,
        // QuickNode
        "too many results" | "result window too large" => true,
        // Generic
        "too many events" | "exceeded maximum number of events" | "block range too large" => true,
        _ => false,
    }
}
