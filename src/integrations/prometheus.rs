use crate::metrics::{LiveMetrics, StatsCollector};
use anyhow::Result;
use prometheus::{Encoder, Gauge, IntCounter, IntGauge, Opts, Registry, TextEncoder};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct PrometheusMetrics {
    registry: Registry,

    // Request metrics
    requests_total: IntCounter,
    requests_successful: IntCounter,
    requests_failed: IntCounter,

    // Performance metrics
    requests_per_second: Gauge,

    // Response time percentiles
    response_time_p50: Gauge,
    response_time_p90: Gauge,
    response_time_p95: Gauge,
    response_time_p99: Gauge,

    // System metrics
    active_connections: IntGauge,
    bytes_received_total: IntCounter,

    // Error metrics
    error_rate: Gauge,

    // Status code counters
    status_code_counters: Arc<RwLock<HashMap<u16, IntCounter>>>,
}

impl PrometheusMetrics {
    pub fn new() -> Result<Self> {
        let registry = Registry::new();

        // Request metrics
        let requests_total = IntCounter::with_opts(
            Opts::new("pulzr_requests_total", "Total number of requests sent").namespace("pulzr"),
        )?;
        registry.register(Box::new(requests_total.clone()))?;

        let requests_successful = IntCounter::with_opts(
            Opts::new(
                "pulzr_requests_successful_total",
                "Total number of successful requests",
            )
            .namespace("pulzr"),
        )?;
        registry.register(Box::new(requests_successful.clone()))?;

        let requests_failed = IntCounter::with_opts(
            Opts::new(
                "pulzr_requests_failed_total",
                "Total number of failed requests",
            )
            .namespace("pulzr"),
        )?;
        registry.register(Box::new(requests_failed.clone()))?;

        // Performance metrics

        let requests_per_second = Gauge::with_opts(
            Opts::new("pulzr_requests_per_second", "Current requests per second")
                .namespace("pulzr"),
        )?;
        registry.register(Box::new(requests_per_second.clone()))?;

        // Response time percentiles
        let response_time_p50 = Gauge::with_opts(
            Opts::new(
                "pulzr_response_time_p50_ms",
                "50th percentile response time in milliseconds",
            )
            .namespace("pulzr"),
        )?;
        registry.register(Box::new(response_time_p50.clone()))?;

        let response_time_p90 = Gauge::with_opts(
            Opts::new(
                "pulzr_response_time_p90_ms",
                "90th percentile response time in milliseconds",
            )
            .namespace("pulzr"),
        )?;
        registry.register(Box::new(response_time_p90.clone()))?;

        let response_time_p95 = Gauge::with_opts(
            Opts::new(
                "pulzr_response_time_p95_ms",
                "95th percentile response time in milliseconds",
            )
            .namespace("pulzr"),
        )?;
        registry.register(Box::new(response_time_p95.clone()))?;

        let response_time_p99 = Gauge::with_opts(
            Opts::new(
                "pulzr_response_time_p99_ms",
                "99th percentile response time in milliseconds",
            )
            .namespace("pulzr"),
        )?;
        registry.register(Box::new(response_time_p99.clone()))?;

        // System metrics
        let active_connections = IntGauge::with_opts(
            Opts::new("pulzr_active_connections", "Number of active connections")
                .namespace("pulzr"),
        )?;
        registry.register(Box::new(active_connections.clone()))?;

        let bytes_received_total = IntCounter::with_opts(
            Opts::new("pulzr_bytes_received_total", "Total bytes received").namespace("pulzr"),
        )?;
        registry.register(Box::new(bytes_received_total.clone()))?;

        // Error metrics
        let error_rate = Gauge::with_opts(
            Opts::new("pulzr_error_rate_percent", "Current error rate percentage")
                .namespace("pulzr"),
        )?;
        registry.register(Box::new(error_rate.clone()))?;

        Ok(Self {
            registry,
            requests_total,
            requests_successful,
            requests_failed,
            requests_per_second,
            response_time_p50,
            response_time_p90,
            response_time_p95,
            response_time_p99,
            active_connections,
            bytes_received_total,
            error_rate,
            status_code_counters: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    pub async fn update_metrics(&self, metrics: &LiveMetrics) -> Result<()> {
        // Update basic counters
        self.requests_total.reset();
        self.requests_total.inc_by(metrics.requests_sent);

        self.requests_successful.reset();
        self.requests_successful
            .inc_by(metrics.requests_sent - metrics.requests_failed);

        self.requests_failed.reset();
        self.requests_failed.inc_by(metrics.requests_failed);

        // Update performance metrics
        self.requests_per_second.set(metrics.current_rps);

        // Update response time percentiles
        self.response_time_p50.set(metrics.p50_response_time as f64);
        self.response_time_p90.set(metrics.p90_response_time as f64);
        self.response_time_p95.set(metrics.p95_response_time as f64);
        self.response_time_p99.set(metrics.p99_response_time as f64);

        // Update system metrics
        self.active_connections
            .set(metrics.active_connections as i64);

        self.bytes_received_total.reset();
        self.bytes_received_total.inc_by(metrics.bytes_received);

        // Update error rate
        let error_rate = if metrics.requests_sent > 0 {
            (metrics.requests_failed as f64 / metrics.requests_sent as f64) * 100.0
        } else {
            0.0
        };
        self.error_rate.set(error_rate);

        // Update status code counters
        let mut status_counters = self.status_code_counters.write().await;
        for (status_code, count) in &metrics.status_codes {
            let counter = status_counters.entry(*status_code).or_insert_with(|| {
                let counter = IntCounter::with_opts(
                    Opts::new(
                        format!("pulzr_status_code_{status_code}_total"),
                        format!("Total requests with status code {status_code}"),
                    )
                    .namespace("pulzr"),
                )
                .unwrap();

                // Register the counter with the registry
                if let Err(e) = self.registry.register(Box::new(counter.clone())) {
                    eprintln!("Failed to register status code counter: {e}");
                }

                counter
            });

            counter.reset();
            counter.inc_by(*count);
        }

        Ok(())
    }

    pub fn get_registry(&self) -> &Registry {
        &self.registry
    }

    pub fn export_metrics(&self) -> Result<String> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer)?;
        Ok(String::from_utf8(buffer)?)
    }
}

pub struct PrometheusExporter {
    metrics: Arc<PrometheusMetrics>,
    stats_collector: Arc<StatsCollector>,
}

impl PrometheusExporter {
    pub fn new(stats_collector: Arc<StatsCollector>) -> Result<Self> {
        let metrics = Arc::new(PrometheusMetrics::new()?);

        Ok(Self {
            metrics,
            stats_collector,
        })
    }

    pub async fn update_from_stats(&self) -> Result<()> {
        let live_metrics = self.stats_collector.get_live_metrics().await;
        self.metrics.update_metrics(&live_metrics).await
    }

    pub fn export_metrics(&self) -> Result<String> {
        self.metrics.export_metrics()
    }

    pub fn get_registry(&self) -> &Registry {
        self.metrics.get_registry()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::LatencyHistogram;

    #[tokio::test]
    async fn test_prometheus_metrics_creation() {
        let metrics = PrometheusMetrics::new().unwrap();
        assert_eq!(metrics.requests_total.get(), 0);
        assert_eq!(metrics.requests_per_second.get(), 0.0);
    }

    #[tokio::test]
    async fn test_metrics_update() {
        let metrics = PrometheusMetrics::new().unwrap();

        let test_metrics = LiveMetrics {
            requests_sent: 100,
            requests_completed: 95,
            requests_failed: 5,
            current_rps: 10.5,
            avg_response_time: 250.0,
            min_response_time: 100,
            max_response_time: 500,
            p50_response_time: 200,
            p90_response_time: 400,
            p95_response_time: 450,
            p99_response_time: 500,
            active_connections: 5,
            queue_size: 0,
            bytes_received: 1024,
            status_codes: std::collections::HashMap::from([(200, 95), (500, 5)]),
            errors: std::collections::HashMap::new(),
            latency_histogram: LatencyHistogram::new(),
            active_alerts: Vec::new(),
        };

        metrics.update_metrics(&test_metrics).await.unwrap();

        assert_eq!(metrics.requests_total.get(), 100);
        assert_eq!(metrics.requests_successful.get(), 95);
        assert_eq!(metrics.requests_failed.get(), 5);
        assert_eq!(metrics.requests_per_second.get(), 10.5);
        assert_eq!(metrics.response_time_p50.get(), 200.0);
        assert_eq!(metrics.error_rate.get(), 5.0);
    }

    #[test]
    fn test_metrics_export() {
        let metrics = PrometheusMetrics::new().unwrap();
        let export = metrics.export_metrics().unwrap();

        // Check that the export contains some expected metrics
        assert!(export.contains("pulzr_requests_total"));
        assert!(export.contains("pulzr_requests_per_second"));
        assert!(export.contains("pulzr_response_time_p50_ms"));
    }
}
