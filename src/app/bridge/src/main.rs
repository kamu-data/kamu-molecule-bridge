use std::sync::Arc;

use alloy::providers::fillers::ChainIdFiller;
use alloy::providers::{DynProvider, Provider};
use color_eyre::eyre;
use dotenv::dotenv;
use kamu_molecule_bridge::prelude::*;
use kamu_node_api_client::KamuNodeApiClientImpl;
use multisig_safe_wallet::services::SafeWalletApiService;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BINARY_NAME: &str = env!("CARGO_PKG_NAME");
const DEFAULT_RUST_LOG: &str =
    "debug,alloy_transport_http=info,alloy_rpc_client=info,reqwest=info,hyper=info,h2=info";

fn main() -> eyre::Result<()> {
    // TODO Warning: SpanTrace capture is Unsupported
    //      Ensure that you've setup a tracing-error ErrorLayer and the semver versions are compatible
    //      - https://github.com/eyre-rs/eyre/tree/master/color-eyre#disabling-spantrace-capture-by-default
    //      - https://github.com/eyre-rs/color-eyre/issues/32
    //      - https://github.com/eyre-rs/eyre/tree/master/color-spantrace
    color_eyre::install()?;

    dotenv()?;

    init_tls();

    let _guard = init_observability();

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

    let kamu_node_api_client = Arc::new(KamuNodeApiClientImpl::new(
        config.kamu_node_gql_api_endpoint.clone(),
        config.kamu_node_token.clone(),
        config.molecule_projects_dataset_alias.clone(),
    ));

    let mut app = App::new(
        config,
        provider,
        &safe_wallet_api_service,
        kamu_node_api_client,
    );

    tracing::info!(version = VERSION, "Running {BINARY_NAME}");

    let shutdown_requested = trap_signals();

    app.run(shutdown_requested).await?;

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

fn init_tls() {
    // TODO: Currently we are compiling `rustls` with both `ring` and `aws-cl-rs`
    // backends and since v0.23 `rustls` requires to disambiguate between which
    // one to use. Eventually we should unify all dependencies around the same
    // backend, but a number of them don't yet expose the necessary feature flags.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Could not install default TLS provider");
}

fn init_observability() -> observability::init::Guard {
    // Configure tracing and opentelemetry
    let guard = observability::init::auto(
        observability::config::Config::from_env_with_prefix("KAMU_OTEL_")
            .with_service_name(BINARY_NAME)
            .with_service_version(VERSION)
            .with_default_log_levels(DEFAULT_RUST_LOG),
    );

    // Redirect panics to tracing
    observability::panic_handler::set_hook_trace_panics(false);

    guard
}

async fn trap_signals() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::warn!("SIGINT signal received, shutting down gracefully");
        },
        _ = terminate => {
            tracing::warn!("SIGTERM signal received, shutting down gracefully");
        },
    }
}
