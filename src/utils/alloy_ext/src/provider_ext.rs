use alloy::providers::{DynProvider, Provider};
use alloy::rpc::types::{Filter, Log};
use async_trait::async_trait;
use color_eyre::eyre;

#[async_trait]
pub trait ProviderExt {
    async fn get_logs_ext(&self, filter: &Filter) -> eyre::Result<Vec<Log>>;
}

#[async_trait]
impl ProviderExt for DynProvider {
    async fn get_logs_ext(&self, filter: &Filter) -> color_eyre::Result<Vec<Log>> {
        // TODO: Handle RPC errors (too many events)
        let logs = self.get_logs(filter).await?;

        Ok(logs)
    }
}
