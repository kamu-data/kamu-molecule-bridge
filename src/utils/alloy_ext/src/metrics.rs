use std::{
    fmt::Debug,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use alloy::{
    rpc::json_rpc::{RequestPacket, ResponsePacket},
    transports::TransportError,
};
use tower::{Layer, Service};

pub struct MetricsLayer {
    metric_requests_num_total: prometheus::IntCounter,
    metric_errors_num_total: prometheus::IntCounter,
}

impl MetricsLayer {
    pub fn new(
        metric_requests_num_total: prometheus::IntCounter,
        metric_errors_num_total: prometheus::IntCounter,
    ) -> Self {
        Self {
            metric_requests_num_total,
            metric_errors_num_total,
        }
    }
}

// A tower::Layer that reports Prometheus metrics for RPC calls.
impl<S> Layer<S> for MetricsLayer {
    type Service = MetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        MetricsService {
            inner,
            metric_requests_num_total: self.metric_requests_num_total.clone(),
            metric_errors_num_total: self.metric_errors_num_total.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MetricsService<S> {
    inner: S,
    metric_requests_num_total: prometheus::IntCounter,
    metric_errors_num_total: prometheus::IntCounter,
}

impl<S> Service<RequestPacket> for MetricsService<S>
where
    S: Service<RequestPacket, Response = ResponsePacket, Error = TransportError>,
    S::Future: Send + 'static,
    S::Response: Send + 'static + Debug,
    S::Error: Send + 'static + Debug,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: RequestPacket) -> Self::Future {
        self.metric_requests_num_total.inc();

        let fut = self.inner.call(req);
        let metric_errors_num_total = self.metric_errors_num_total.clone();

        Box::pin(async move {
            match fut.await {
                Ok(res) => Ok(res),
                Err(err) => {
                    metric_errors_num_total.inc();
                    Err(err)
                }
            }
        })
    }
}
