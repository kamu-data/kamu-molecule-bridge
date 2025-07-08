use alloy::providers::fillers::ChainIdFiller;
use alloy::providers::{DynProvider, Provider};
use color_eyre::eyre;
use dotenv::dotenv;
use kamu_molecule_bridge::prelude::*;
use multisig_safe_wallet::services::SafeWalletApiService;

fn main() -> eyre::Result<()> {
    color_eyre::install()?;

    // TODO: Tracing initialization
    dotenv()?;

    let config = Config::builder()
        .env()
        // TODO: Add support for config file
        // .file(&args.config)
        .load()?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(main_async(config))
}

async fn main_async(config: Config) -> eyre::Result<()> {
    let provider = build_rpc_client(&config).await?;
    let chain_id = provider.get_chain_id().await?;
    let safe_wallet_api_service = SafeWalletApiService::new_from_chain_id(chain_id)?;

    let app = App::new(config, provider, &safe_wallet_api_service);

    app.run().await?;

    Ok(())
}

async fn build_rpc_client(config: &Config) -> eyre::Result<DynProvider> {
    let provider = alloy::providers::ProviderBuilder::new()
        // We do not work with transactions, so we disable all filters ...
        .disable_recommended_fillers()
        // ... except caching filter for ChainId.
        .filler(ChainIdFiller::default())
        .connect(&config.rpc_url)
        .await?
        .erased();

    Ok(provider)
}
