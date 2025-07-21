use std::sync::Arc;

use alloy::providers::fillers::ChainIdFiller;
use alloy::providers::{DynProvider, Provider};
use clap::Parser as _;
use color_eyre::eyre;
use kamu_molecule_bridge::cli::Cli;
use kamu_molecule_bridge::metrics::BridgeMetrics;
use kamu_molecule_bridge::prelude::*;
use kamu_node_api_client::KamuNodeApiClientImpl;
use multisig_safe_wallet::services::SafeWalletApiService;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const BINARY_NAME: &str = env!("CARGO_PKG_NAME");
const DEFAULT_RUST_LOG: &str =
    "debug,alloy_transport_http=info,alloy_rpc_client=info,reqwest=info,hyper=info,h2=info";

// The job of main() is to load env vars and config and start the runtime
fn main() -> eyre::Result<()> {
    init_error_reporting()?;

    // FIXME: Not handling errors due to poor API that doesn't allow to easily
    // differentiate .env file's ansence from other errors
    dotenv::dotenv().ok();

    let args = Cli::parse();

    init_tls();

    // Loads configuration from env and config file
    // Config file is optional.
    // Environment variables take precedence over the config.
    let config = Config::builder().env().file(&args.config).load()?;

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(main_async(config, args))
}

// The job of main_async() is to initialize observability and redirect unhandled errors to tracing
async fn main_async(config: Config, args: Cli) -> eyre::Result<()> {
    let observability = init_observability();

    match main_app(config, args).await {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::error!(
                error = ?err,
                error_msg = %err,
                "Unhandled error",
            );

            // NOTE: The error was already reported to tracing, so we will flush tracing
            // and exit the process with error code not to output it twice.
            drop(observability);
            std::process::exit(1);
        }
    }
}

async fn main_app(config: Config, args: Cli) -> eyre::Result<()> {
    let (metrics_registry, metrics) = init_metrics(&config)?;

    let rpc_client = build_rpc_client(&config, &metrics).await?;

    let safe_wallet_api_service =
        Arc::new(SafeWalletApiService::new_from_chain_id(config.chain_id)?);

    let kamu_node_api_client = build_kamu_node_client(&config, &metrics);

    tracing::info!(version = VERSION, ?config, ?args, "Running {BINARY_NAME}");

    let shutdown_requested = trap_signals();

    let mut app = App::new(
        config,
        rpc_client,
        safe_wallet_api_service,
        kamu_node_api_client,
        metrics,
        metrics_registry,
    );

    app.run(shutdown_requested).await
}

async fn build_rpc_client(config: &Config, metrics: &BridgeMetrics) -> eyre::Result<DynProvider> {
    let client = alloy::rpc::client::ClientBuilder::default()
        .layer(alloy_ext::metrics::MetricsLayer::new(
            metrics.evm_rpc_requests_num_total.clone(),
            metrics.evm_rpc_errors_num_total.clone(),
        ))
        .layer(alloy_ext::tracing::TracingLayer)
        .connect(&config.rpc_url)
        .await?;

    let provider = alloy::providers::ProviderBuilder::new()
        // We do not work with transactions, so we disable all filters ...
        .disable_recommended_fillers()
        // ... except caching filter for ChainId.
        .filler(ChainIdFiller::default())
        .connect_client(client)
        .erased();

    // Check that we are looking at the right chain
    let actual_chain_id = provider.get_chain_id().await?;
    if actual_chain_id != config.chain_id {
        eyre::bail!(
            "Expected to communicate with chain ID '{}' but RPC returned '{actual_chain_id}' instead",
            config.chain_id,
        );
    }

    Ok(provider)
}

fn build_kamu_node_client(config: &Config, metrics: &BridgeMetrics) -> Arc<KamuNodeApiClientImpl> {
    Arc::new(KamuNodeApiClientImpl::new(
        config.kamu_node_gql_api_endpoint.clone(),
        config.kamu_node_token.clone(),
        config.molecule_projects_dataset_alias.clone(),
        metrics.kamu_gql_requests_num_total.clone(),
        metrics.kamu_gql_errors_num_total.clone(),
    ))
}

fn init_error_reporting() -> eyre::Result<()> {
    use observability::config::Mode;

    // Use blank theme when in service mode to avoid ANSII colors in tracing
    let theme = match observability::init::auto_detect_mode() {
        Mode::Dev => color_eyre::config::Theme::dark(),
        Mode::Service => color_eyre::config::Theme::new(),
    };

    color_eyre::config::HookBuilder::default()
        .theme(theme)
        .install()?;

    Ok(())
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

fn init_metrics(config: &Config) -> eyre::Result<(prometheus::Registry, BridgeMetrics)> {
    let metrics = BridgeMetrics::new(config.chain_id);

    let metrics_registry =
        prometheus::Registry::new_custom(Some("kamu_molecule_bridge".into()), None).unwrap();
    metrics.register(&metrics_registry)?;

    Ok((metrics_registry, metrics))
}

async fn trap_signals() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    tokio::select! {
        _ = ctrl_c => {
            tracing::warn!("SIGINT signal received, shutting down gracefully");
        },
        _ = terminate => {
            tracing::warn!("SIGTERM signal received, shutting down gracefully");
        },
    }
}
