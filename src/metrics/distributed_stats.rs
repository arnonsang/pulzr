use super::stats::{Alert, FinalSummary, LatencyHistogram, LiveMetrics};
use crate::integrations::websocket::{WebSocketMessage, WorkerLoad};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Metrics aggregated from multiple workers in a distributed load test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedMetrics {
    pub total_workers: usize,
    pub active_workers: usize,
    pub global_metrics: LiveMetrics,
    pub worker_metrics: HashMap<String, WorkerMetrics>,
    pub aggregation_timestamp: DateTime<Utc>,
}

/// Metrics for a specific worker in the distributed test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerMetrics {
    pub worker_id: String,
    pub status: String,
    pub metrics: LiveMetrics,
    pub load: WorkerLoad,
    pub last_update: DateTime<Utc>,
    pub connection_time: DateTime<Utc>,
}

/// Distributed stats collector that aggregates metrics from multiple workers
pub struct DistributedStatsCollector {
    worker_metrics: Arc<RwLock<HashMap<String, WorkerMetrics>>>,
    global_histogram: Arc<RwLock<LatencyHistogram>>,
    aggregated_metrics: Arc<RwLock<AggregatedMetrics>>,
    websocket_sender: Option<broadcast::Sender<WebSocketMessage>>,
    start_time: DateTime<Utc>,
}

impl DistributedStatsCollector {
    pub fn new() -> Self {
        let initial_aggregated = AggregatedMetrics {
            total_workers: 0,
            active_workers: 0,
            global_metrics: LiveMetrics {
                requests_sent: 0,
                requests_completed: 0,
                requests_failed: 0,
                current_rps: 0.0,
                avg_response_time: 0.0,
                min_response_time: 0,
                max_response_time: 0,
                p50_response_time: 0,
                p90_response_time: 0,
                p95_response_time: 0,
                p99_response_time: 0,
                active_connections: 0,
                queue_size: 0,
                bytes_received: 0,
                status_codes: HashMap::new(),
                errors: HashMap::new(),
                latency_histogram: LatencyHistogram::new(),
                active_alerts: Vec::new(),
            },
            worker_metrics: HashMap::new(),
            aggregation_timestamp: Utc::now(),
        };

        Self {
            worker_metrics: Arc::new(RwLock::new(HashMap::new())),
            global_histogram: Arc::new(RwLock::new(LatencyHistogram::new())),
            aggregated_metrics: Arc::new(RwLock::new(initial_aggregated)),
            websocket_sender: None,
            start_time: Utc::now(),
        }
    }

    /// Set WebSocket sender for broadcasting aggregated metrics
    pub fn with_websocket_sender(mut self, sender: broadcast::Sender<WebSocketMessage>) -> Self {
        self.websocket_sender = Some(sender);
        self
    }

    /// Update metrics for a specific worker
    pub async fn update_worker_metrics(
        &self,
        worker_id: String,
        metrics: LiveMetrics,
        load: WorkerLoad,
        status: String,
    ) {
        let now = Utc::now();

        let worker_metrics = WorkerMetrics {
            worker_id: worker_id.clone(),
            status,
            metrics,
            load,
            last_update: now,
            connection_time: now, // This should be set when worker first connects
        };

        // Update worker-specific metrics
        {
            let mut workers = self.worker_metrics.write().await;
            workers.insert(worker_id, worker_metrics);
        }

        // Recalculate aggregated metrics
        self.recalculate_aggregated_metrics().await;

        // Broadcast updated metrics
        self.broadcast_aggregated_metrics().await;
    }

    /// Remove a worker from tracking (when it disconnects)
    pub async fn remove_worker(&self, worker_id: &str) {
        {
            let mut workers = self.worker_metrics.write().await;
            workers.remove(worker_id);
        }

        // Recalculate aggregated metrics
        self.recalculate_aggregated_metrics().await;

        // Broadcast updated metrics
        self.broadcast_aggregated_metrics().await;
    }

    /// Add a new worker to tracking (when it connects)
    pub async fn add_worker(&self, worker_id: String, connection_time: DateTime<Utc>) {
        let worker_metrics = WorkerMetrics {
            worker_id: worker_id.clone(),
            status: "Connected".to_string(),
            metrics: LiveMetrics {
                requests_sent: 0,
                requests_completed: 0,
                requests_failed: 0,
                current_rps: 0.0,
                avg_response_time: 0.0,
                min_response_time: 0,
                max_response_time: 0,
                p50_response_time: 0,
                p90_response_time: 0,
                p95_response_time: 0,
                p99_response_time: 0,
                active_connections: 0,
                queue_size: 0,
                bytes_received: 0,
                status_codes: HashMap::new(),
                errors: HashMap::new(),
                latency_histogram: LatencyHistogram::new(),
                active_alerts: Vec::new(),
            },
            load: WorkerLoad {
                current_rps: 0.0,
                active_connections: 0,
                memory_usage_mb: 0,
                cpu_usage_percent: 0.0,
                total_requests_sent: 0,
                errors_count: 0,
            },
            last_update: connection_time,
            connection_time,
        };

        {
            let mut workers = self.worker_metrics.write().await;
            workers.insert(worker_id, worker_metrics);
        }

        self.recalculate_aggregated_metrics().await;
    }

    /// Recalculate global aggregated metrics from all workers
    async fn recalculate_aggregated_metrics(&self) {
        let workers_guard = self.worker_metrics.read().await;
        let worker_count = workers_guard.len();
        let active_worker_count = workers_guard
            .values()
            .filter(|w| w.status == "Running" || w.status == "Connected")
            .count();

        // Aggregate metrics across all workers
        let mut global_requests_sent = 0u64;
        let mut global_requests_completed = 0u64;
        let mut global_requests_failed = 0u64;
        let mut global_bytes_received = 0u64;
        let mut global_active_connections = 0u64;
        let mut global_status_codes = HashMap::new();
        let mut global_errors = HashMap::new();
        let mut global_alerts = Vec::new();
        let _all_response_times: Vec<u64> = Vec::new(); // Reserved for future use
        let mut total_rps = 0.0;

        // Aggregate histogram data
        let mut global_histogram = LatencyHistogram::new();

        for worker in workers_guard.values() {
            let metrics = &worker.metrics;

            global_requests_sent += metrics.requests_sent;
            global_requests_completed += metrics.requests_completed;
            global_requests_failed += metrics.requests_failed;
            global_bytes_received += metrics.bytes_received;
            global_active_connections += metrics.active_connections;
            total_rps += metrics.current_rps;

            // Aggregate status codes
            for (code, count) in &metrics.status_codes {
                *global_status_codes.entry(*code).or_insert(0) += count;
            }

            // Aggregate errors
            for (error, count) in &metrics.errors {
                *global_errors.entry(error.clone()).or_insert(0) += count;
            }

            // Aggregate alerts (deduplicate by type)
            for alert in &metrics.active_alerts {
                if !global_alerts
                    .iter()
                    .any(|a: &Alert| a.alert_type == alert.alert_type)
                {
                    global_alerts.push(alert.clone());
                }
            }

            // Aggregate histogram buckets
            for (i, count) in metrics.latency_histogram.buckets.iter().enumerate() {
                if i < global_histogram.buckets.len() {
                    global_histogram.buckets[i] += count;
                    global_histogram.total_count += count;
                }
            }
            global_histogram.max_value = global_histogram
                .max_value
                .max(metrics.latency_histogram.max_value);
        }

        // Calculate global percentiles and averages
        let (avg_response_time, min_response_time, max_response_time, p50, p90, p95, p99) =
            if global_requests_completed > 0 {
                // For distributed metrics, we'll use weighted averages from workers
                let mut total_avg_weighted = 0.0;
                let mut total_weight = 0.0;
                let mut global_min = u64::MAX;
                let mut global_max = 0u64;

                for worker in workers_guard.values() {
                    let weight = worker.metrics.requests_completed as f64;
                    if weight > 0.0 {
                        total_avg_weighted += worker.metrics.avg_response_time * weight;
                        total_weight += weight;
                        global_min = global_min.min(worker.metrics.min_response_time);
                        global_max = global_max.max(worker.metrics.max_response_time);
                    }
                }

                let weighted_avg = if total_weight > 0.0 {
                    total_avg_weighted / total_weight
                } else {
                    0.0
                };

                // For percentiles in distributed environment, use weighted average of worker percentiles
                let mut p50_weighted = 0.0;
                let mut p90_weighted = 0.0;
                let mut p95_weighted = 0.0;
                let mut p99_weighted = 0.0;
                let mut percentile_weight = 0.0;

                for worker in workers_guard.values() {
                    let weight = worker.metrics.requests_completed as f64;
                    if weight > 0.0 {
                        p50_weighted += worker.metrics.p50_response_time as f64 * weight;
                        p90_weighted += worker.metrics.p90_response_time as f64 * weight;
                        p95_weighted += worker.metrics.p95_response_time as f64 * weight;
                        p99_weighted += worker.metrics.p99_response_time as f64 * weight;
                        percentile_weight += weight;
                    }
                }

                let p50 = if percentile_weight > 0.0 {
                    (p50_weighted / percentile_weight) as u64
                } else {
                    0
                };
                let p90 = if percentile_weight > 0.0 {
                    (p90_weighted / percentile_weight) as u64
                } else {
                    0
                };
                let p95 = if percentile_weight > 0.0 {
                    (p95_weighted / percentile_weight) as u64
                } else {
                    0
                };
                let p99 = if percentile_weight > 0.0 {
                    (p99_weighted / percentile_weight) as u64
                } else {
                    0
                };

                (weighted_avg, global_min, global_max, p50, p90, p95, p99)
            } else {
                (0.0, 0, 0, 0, 0, 0, 0)
            };

        let global_metrics = LiveMetrics {
            requests_sent: global_requests_sent,
            requests_completed: global_requests_completed,
            requests_failed: global_requests_failed,
            current_rps: total_rps,
            avg_response_time,
            min_response_time,
            max_response_time,
            p50_response_time: p50,
            p90_response_time: p90,
            p95_response_time: p95,
            p99_response_time: p99,
            active_connections: global_active_connections,
            queue_size: 0, // Queue size is worker-specific
            bytes_received: global_bytes_received,
            status_codes: global_status_codes,
            errors: global_errors,
            latency_histogram: global_histogram.clone(),
            active_alerts: global_alerts,
        };

        // Update global histogram
        {
            let mut histogram_guard = self.global_histogram.write().await;
            *histogram_guard = global_histogram;
        }

        // Update aggregated metrics
        let aggregated = AggregatedMetrics {
            total_workers: worker_count,
            active_workers: active_worker_count,
            global_metrics,
            worker_metrics: workers_guard.clone(),
            aggregation_timestamp: Utc::now(),
        };

        {
            let mut aggregated_guard = self.aggregated_metrics.write().await;
            *aggregated_guard = aggregated;
        }
    }

    /// Broadcast aggregated metrics via WebSocket
    async fn broadcast_aggregated_metrics(&self) {
        if let Some(sender) = &self.websocket_sender {
            let aggregated = self.aggregated_metrics.read().await;

            let message = WebSocketMessage::AggregatedMetrics {
                timestamp: Utc::now(),
                aggregated_metrics: aggregated.clone(),
            };

            let _ = sender.send(message);
        }
    }

    /// Get current aggregated metrics
    pub async fn get_aggregated_metrics(&self) -> AggregatedMetrics {
        self.aggregated_metrics.read().await.clone()
    }

    /// Get metrics for a specific worker
    pub async fn get_worker_metrics(&self, worker_id: &str) -> Option<WorkerMetrics> {
        let workers = self.worker_metrics.read().await;
        workers.get(worker_id).cloned()
    }

    /// Get all connected workers
    pub async fn get_all_workers(&self) -> Vec<WorkerMetrics> {
        let workers = self.worker_metrics.read().await;
        workers.values().cloned().collect()
    }

    /// Get global aggregated live metrics (compatible with existing StatsCollector interface)
    pub async fn get_live_metrics(&self) -> LiveMetrics {
        let aggregated = self.aggregated_metrics.read().await;
        aggregated.global_metrics.clone()
    }

    /// Get final summary for distributed test
    pub async fn get_final_summary(&self) -> FinalSummary {
        let aggregated = self.aggregated_metrics.read().await;
        let global = &aggregated.global_metrics;
        let end_time = Utc::now();
        let test_duration = (end_time - self.start_time).num_seconds() as f64;

        let avg_rps = if test_duration > 0.0 {
            global.requests_completed as f64 / test_duration
        } else {
            global.current_rps
        };

        // Extract user agents from worker metrics (simplified)
        let mut user_agents = HashMap::new();
        for worker in aggregated.worker_metrics.values() {
            // This is a simplified approach since we don't have detailed user agent info
            // In a real implementation, workers would need to report this data
            user_agents.insert(
                format!("worker-{}", worker.worker_id),
                worker.metrics.requests_completed,
            );
        }

        FinalSummary {
            total_requests: global.requests_sent,
            successful_requests: global.requests_completed,
            failed_requests: global.requests_failed,
            test_duration_secs: test_duration,
            avg_rps,
            avg_response_time: global.avg_response_time,
            min_response_time: global.min_response_time,
            max_response_time: global.max_response_time,
            p50_response_time: global.p50_response_time,
            p95_response_time: global.p95_response_time,
            p99_response_time: global.p99_response_time,
            total_bytes_received: global.bytes_received,
            status_codes: global.status_codes.clone(),
            errors: global.errors.clone(),
            user_agents_used: user_agents,
        }
    }

    /// Get worker count and status summary
    pub async fn get_worker_summary(&self) -> WorkerSummary {
        let workers = self.worker_metrics.read().await;
        let total = workers.len();
        let mut by_status = HashMap::new();

        for worker in workers.values() {
            *by_status.entry(worker.status.clone()).or_insert(0) += 1;
        }

        WorkerSummary {
            total_workers: total,
            workers_by_status: by_status,
            last_updated: Utc::now(),
        }
    }
}

impl Default for DistributedStatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of worker status in distributed test
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerSummary {
    pub total_workers: usize,
    pub workers_by_status: HashMap<String, usize>,
    pub last_updated: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::websocket::WorkerLoad;

    #[tokio::test]
    async fn test_distributed_stats_collector_creation() {
        let collector = DistributedStatsCollector::new();
        let metrics = collector.get_aggregated_metrics().await;

        assert_eq!(metrics.total_workers, 0);
        assert_eq!(metrics.active_workers, 0);
        assert_eq!(metrics.global_metrics.requests_sent, 0);
    }

    #[tokio::test]
    async fn test_add_and_remove_worker() {
        let collector = DistributedStatsCollector::new();
        let worker_id = "test-worker-1".to_string();
        let connection_time = Utc::now();

        // Add worker
        collector
            .add_worker(worker_id.clone(), connection_time)
            .await;

        let metrics = collector.get_aggregated_metrics().await;
        assert_eq!(metrics.total_workers, 1);
        assert!(metrics.worker_metrics.contains_key(&worker_id));

        // Remove worker
        collector.remove_worker(&worker_id).await;

        let metrics = collector.get_aggregated_metrics().await;
        assert_eq!(metrics.total_workers, 0);
        assert!(!metrics.worker_metrics.contains_key(&worker_id));
    }

    #[tokio::test]
    async fn test_update_worker_metrics() {
        let collector = DistributedStatsCollector::new();
        let worker_id = "test-worker-1".to_string();

        // Add worker first
        collector.add_worker(worker_id.clone(), Utc::now()).await;

        // Create test metrics
        let test_metrics = LiveMetrics {
            requests_sent: 100,
            requests_completed: 95,
            requests_failed: 5,
            current_rps: 10.0,
            avg_response_time: 150.0,
            min_response_time: 50,
            max_response_time: 300,
            p50_response_time: 140,
            p90_response_time: 200,
            p95_response_time: 250,
            p99_response_time: 290,
            active_connections: 5,
            queue_size: 0,
            bytes_received: 1024,
            status_codes: HashMap::new(),
            errors: HashMap::new(),
            latency_histogram: LatencyHistogram::new(),
            active_alerts: Vec::new(),
        };

        let test_load = WorkerLoad {
            current_rps: 10.0,
            active_connections: 5,
            memory_usage_mb: 256,
            cpu_usage_percent: 25.0,
            total_requests_sent: 100,
            errors_count: 5,
        };

        // Update worker metrics
        collector
            .update_worker_metrics(
                worker_id.clone(),
                test_metrics.clone(),
                test_load,
                "Running".to_string(),
            )
            .await;

        // Verify aggregated metrics
        let aggregated = collector.get_aggregated_metrics().await;
        assert_eq!(aggregated.global_metrics.requests_sent, 100);
        assert_eq!(aggregated.global_metrics.requests_completed, 95);
        assert_eq!(aggregated.global_metrics.requests_failed, 5);
        assert_eq!(aggregated.global_metrics.current_rps, 10.0);
        assert_eq!(aggregated.active_workers, 1);
    }

    #[tokio::test]
    async fn test_multiple_worker_aggregation() {
        let collector = DistributedStatsCollector::new();

        // Add multiple workers
        for i in 1..=3 {
            let worker_id = format!("worker-{}", i);
            collector.add_worker(worker_id.clone(), Utc::now()).await;

            let metrics = LiveMetrics {
                requests_sent: 100 * i as u64,
                requests_completed: 95 * i as u64,
                requests_failed: 5 * i as u64,
                current_rps: 10.0 * i as f64,
                avg_response_time: 150.0,
                min_response_time: 50,
                max_response_time: 300,
                p50_response_time: 140,
                p90_response_time: 200,
                p95_response_time: 250,
                p99_response_time: 290,
                active_connections: 5 * i as u64,
                queue_size: 0,
                bytes_received: 1024 * i as u64,
                status_codes: HashMap::new(),
                errors: HashMap::new(),
                latency_histogram: LatencyHistogram::new(),
                active_alerts: Vec::new(),
            };

            let load = WorkerLoad {
                current_rps: 10.0 * i as f64,
                active_connections: 5 * i,
                memory_usage_mb: 256 * i as u64,
                cpu_usage_percent: 25.0 * i as f64,
                total_requests_sent: 100 * i as u64,
                errors_count: 5 * i as u64,
            };

            collector
                .update_worker_metrics(worker_id, metrics, load, "Running".to_string())
                .await;
        }

        // Verify aggregated results
        let aggregated = collector.get_aggregated_metrics().await;
        assert_eq!(aggregated.total_workers, 3);
        assert_eq!(aggregated.active_workers, 3);
        assert_eq!(aggregated.global_metrics.requests_sent, 600); // 100+200+300
        assert_eq!(aggregated.global_metrics.requests_completed, 570); // 95+190+285
        assert_eq!(aggregated.global_metrics.requests_failed, 30); // 5+10+15
        assert_eq!(aggregated.global_metrics.current_rps, 60.0); // 10+20+30
        assert_eq!(aggregated.global_metrics.bytes_received, 6144); // 1024+2048+3072
    }
}
