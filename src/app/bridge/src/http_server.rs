use std::net::SocketAddr;

use color_eyre::eyre;

pub async fn build(
    address: std::net::IpAddr,
    http_port: u16,
) -> eyre::Result<(
    axum::serve::Serve<
        tokio::net::TcpListener,
        axum::routing::IntoMakeService<axum::Router>,
        axum::Router,
    >,
    SocketAddr,
)> {
    let app = axum::Router::new()
        .route("/system/health", axum::routing::get(health_handler))
        .route(
            "/system/metrics",
            axum::routing::get(observability::metrics::metrics_handler_raw),
        )
        .fallback(observability::axum::unknown_fallback_handler);
    //.layer(axum::extract::Extension(catalog));

    let addr = std::net::SocketAddr::from((address, http_port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr().unwrap();

    let server = axum::serve(listener, app.into_make_service());
    Ok((server, local_addr))
}

pub async fn health_handler(
    axum::extract::Query(_args): axum::extract::Query<observability::health::CheckArgs>,
) -> Result<axum::Json<observability::health::CheckSuccess>, observability::health::CheckError> {
    Ok(axum::Json(observability::health::CheckSuccess { ok: true }))
}
