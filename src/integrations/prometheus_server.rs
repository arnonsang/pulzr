use crate::integrations::PrometheusExporter;
use crate::utils::find_available_port;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;

pub struct PrometheusServer {
    port: u16,
    exporter: Arc<PrometheusExporter>,
}

impl PrometheusServer {
    pub fn new(port: u16, exporter: Arc<PrometheusExporter>) -> Self {
        Self { port, exporter }
    }

    pub async fn start(
        &self,
        mut quit_receiver: broadcast::Receiver<()>,
    ) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
        let actual_port = find_available_port(self.port, 50)
            .ok_or_else(|| format!("Could not find available port starting from {}", self.port))?;

        let app = Router::new()
            .route("/metrics", get(metrics_handler))
            .route("/health", get(health_handler))
            .with_state(Arc::clone(&self.exporter));

        let addr = SocketAddr::from(([127, 0, 0, 1], actual_port));

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| format!("Failed to bind Prometheus server to {addr}: {e}"))?;

        println!("📊 Prometheus metrics server listening on: http://{addr}/metrics");

        // Spawn background task to update metrics periodically
        let exporter_clone = Arc::clone(&self.exporter);
        let update_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                if let Err(e) = exporter_clone.update_from_stats().await {
                    eprintln!("Failed to update Prometheus metrics: {e}");
                }
            }
        });

        // Spawn the server
        let server_task = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("Prometheus server error: {e}");
            }
        });

        // Wait for quit signal
        tokio::select! {
            _ = quit_receiver.recv() => {
                println!("Shutting down Prometheus server...");
                update_task.abort();
                server_task.abort();
            }
        }

        Ok(actual_port)
    }
}

async fn metrics_handler(State(exporter): State<Arc<PrometheusExporter>>) -> Response {
    match exporter.export_metrics() {
        Ok(metrics) => (StatusCode::OK, metrics).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to export metrics: {e}"),
        )
            .into_response(),
    }
}

async fn health_handler() -> Response {
    (StatusCode::OK, "OK").into_response()
}
