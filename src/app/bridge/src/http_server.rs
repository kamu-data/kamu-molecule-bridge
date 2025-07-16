use async_trait::async_trait;
use color_eyre::eyre;
use std::net::SocketAddr;
use std::sync::Arc;

pub type HttpServeFuture = axum::serve::Serve<
    tokio::net::TcpListener,
    axum::routing::IntoMakeService<axum::Router>,
    axum::Router,
>;

#[async_trait]
pub trait StateRequester: Send + Sync {
    async fn request_as_json(&self) -> serde_json::Value;
}

pub async fn build(
    address: std::net::IpAddr,
    http_port: u16,
    metrics_reg: prometheus::Registry,
    state_requester: Arc<dyn StateRequester>,
) -> eyre::Result<(HttpServeFuture, SocketAddr)> {
    let app = axum::Router::new()
        .route("/system/health", axum::routing::get(health_handler))
        .route(
            "/system/metrics",
            axum::routing::get(observability::metrics::metrics_handler_raw),
        )
        .route(
            "/system/state",
            axum::routing::get(axum::routing::get(state_handler)),
        )
        .fallback(observability::axum::unknown_fallback_handler)
        .layer(axum::extract::Extension(metrics_reg))
        .layer(axum::extract::Extension(state_requester));

    let addr = std::net::SocketAddr::from((address, http_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    let server = axum::serve(listener, app.into_make_service());
    Ok((server, local_addr))
}

pub async fn health_handler(
    axum::extract::Query(_args): axum::extract::Query<observability::health::CheckArgs>,
) -> Result<axum::Json<observability::health::CheckSuccess>, observability::health::CheckError> {
    Ok(axum::Json(observability::health::CheckSuccess { ok: true }))
}

pub async fn state_handler(
    axum::extract::Extension(state_requester): axum::extract::Extension<Arc<dyn StateRequester>>,
) -> Result<axum::Json<serde_json::Value>, ()> {
    let state_json = state_requester.request_as_json().await;

    Ok(axum::Json(state_json))
}
