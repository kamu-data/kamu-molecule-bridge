use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{Address, B256};
use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::{Filter, Log};
use alloy::transports::{RpcError, TransportErrorKind};
use async_trait::async_trait;
use color_eyre::eyre::{self, ContextCompat, bail};
use std::collections::HashSet;

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
            binary_get_logs(
                self,
                address_window.to_vec(),
                event_signatures.clone(),
                from_block,
                to_block,
                callback,
            )
            .await?;
        }

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
async fn binary_get_logs<F>(
    provider: &DynProvider,
    addresses: Vec<Address>,
    event_signatures: HashSet<B256>,
    from_block: u64,
    to_block: u64,
    callback: &mut F,
) -> eyre::Result<()>
where
    F: FnMut(LogsChunk) -> eyre::Result<()> + Send + Sync,
{
    debug_assert!(to_block >= from_block, "{to_block} >= {from_block}");
    debug_assert!(!addresses.is_empty());
    debug_assert!(!event_signatures.is_empty());

    const MIN_BLOCK_RANGE: u64 = 1;

    let filter = Filter::new()
        .address(addresses.clone())
        .event_signature(event_signatures.clone())
        .from_block(from_block)
        .to_block(to_block);

    let result = provider.get_logs(&filter).await;

    match result {
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
                bail!("Cannot split block range [{from_block}, {to_block}] further: {e}");
            }

            tracing::warn!(
                "Too many events for range [{from_block}, {to_block}], splitting in half",
            );

            // Binary search: split the range in half
            let middle_block = middle_block(from_block, to_block);

            // Process first half
            Box::pin(binary_get_logs(
                provider,
                addresses.clone(),
                event_signatures.clone(),
                from_block,
                middle_block,
                callback,
            ))
            .await?;

            // Process second half
            Box::pin(binary_get_logs(
                provider,
                addresses,
                event_signatures,
                middle_block + 1,
                to_block,
                callback,
            ))
            .await?;

            Ok(())
        }
        Err(e) if is_too_many_requests_error(&e) => {
            bail!("RPC rate limit exceeded (429 Too Many Requests): {e}");
        }
        Err(unexpected_error) => Err(unexpected_error)?,
    }
}

fn is_too_many_events_error(error: &RpcError<TransportErrorKind>) -> bool {
    let error = match error {
        RpcError::ErrorResp(resp) => resp.message.to_lowercase(),
        RpcError::Transport(e) => e.to_string().to_lowercase(),
        _ => return false,
    };

    // Alchemy
    if error.contains("log response size exceeded") || error.contains("query timeout") {
        return true;
    }

    // Infura
    if error.contains("query returned more than") || error.contains("request timed out") {
        return true;
    }

    // QuickNode
    if error.contains("too many results") || error.contains("result window too large") {
        return true;
    }

    // Generic
    if error.contains("too many events")
        || error.contains("exceeded maximum number of events")
        || error.contains("block range too large")
        || error.contains("exceed maximum block range")
    {
        return true;
    }

    false
}

fn is_too_many_requests_error(error: &RpcError<TransportErrorKind>) -> bool {
    match error {
        RpcError::ErrorResp(resp) => resp.is_retry_err(),
        RpcError::Transport(e) => e.is_retry_err(),
        _ => false,
    }
}

fn middle_block(from_block: u64, to_block: u64) -> u64 {
    debug_assert!(to_block >= from_block, "{to_block} >= {from_block}");

    from_block + (to_block - from_block) / 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use rstest::rstest;

    #[rstest]
    #[case(0, 9, 4)]
    #[case(0, 10, 5)]
    #[case(1, 9, 5)]
    #[case(1, 10, 5)]
    #[case(10, 10, 10)]
    #[case(10, 11, 10)]
    #[case(10, 12, 11)]
    fn test_middle_block(#[case] from_block: u64, #[case] to_block: u64, #[case] expected: u64) {
        assert_eq!(expected, middle_block(from_block, to_block));
    }
}
