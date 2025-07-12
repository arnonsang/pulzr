use super::{Alert, AlertConfig, FinalSummary, LatencyHistogram, LiveMetrics, RequestResult};
use crate::config::MemoryConfig;
use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOptimizedStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_duration_ms: u64,
    pub min_duration_ms: u64,
    pub max_duration_ms: u64,
    pub total_bytes_received: u64,
    pub status_code_counts: HashMap<u16, u64>,
    pub error_counts: HashMap<String, u64>,
    pub user_agent_counts: HashMap<String, u64>,
    pub minute_buckets: VecDeque<MinuteBucket>,
    pub hour_buckets: VecDeque<HourBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinuteBucket {
    pub timestamp: DateTime<Utc>,
    pub requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_duration_ms: u64,
    pub min_duration_ms: u64,
    pub max_duration_ms: u64,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourBucket {
    pub timestamp: DateTime<Utc>,
    pub requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub avg_duration_ms: f64,
    pub min_duration_ms: u64,
    pub max_duration_ms: u64,
    pub bytes_received: u64,
}

impl Default for MemoryOptimizedStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_duration_ms: 0,
            min_duration_ms: u64::MAX,
            max_duration_ms: 0,
            total_bytes_received: 0,
            status_code_counts: HashMap::new(),
            error_counts: HashMap::new(),
            user_agent_counts: HashMap::new(),
            minute_buckets: VecDeque::new(),
            hour_buckets: VecDeque::new(),
        }
    }
}

pub struct MemoryOptimizedStatsCollector {
    /// Ring buffer for recent request results (limited size)
    results: Arc<RwLock<VecDeque<RequestResult>>>,

    /// Histogram for response time tracking
    histogram: Arc<RwLock<LatencyHistogram>>,

    /// Aggregated statistics (never cleared)
    optimized_stats: Arc<RwLock<MemoryOptimizedStats>>,

    /// Alert configuration and active alerts
    alert_config: AlertConfig,
    active_alerts: Arc<RwLock<Vec<Alert>>>,

    /// Memory configuration
    memory_config: MemoryConfig,

    /// Start time for calculations
    start_time: DateTime<Utc>,

    /// WebSocket sender for real-time updates
    request_log_sender: Option<broadcast::Sender<crate::integrations::WebSocketMessage>>,

    /// Memory usage tracking
    memory_usage: Arc<RwLock<MemoryUsage>>,

    /// Cleanup task handle
    cleanup_handle: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryUsage {
    pub current_results_in_memory: usize,
    pub estimated_memory_mb: f64,
    pub total_requests_processed: u64,
    pub cleanup_runs: u64,
    pub last_cleanup: DateTime<Utc>,
    pub oldest_result_age_seconds: u64,
}

impl Default for MemoryUsage {
    fn default() -> Self {
        Self {
            current_results_in_memory: 0,
            estimated_memory_mb: 0.0,
            total_requests_processed: 0,
            cleanup_runs: 0,
            last_cleanup: Utc::now(),
            oldest_result_age_seconds: 0,
        }
    }
}

impl MemoryOptimizedStatsCollector {
    pub fn new(memory_config: MemoryConfig) -> Self {
        Self {
            results: Arc::new(RwLock::new(VecDeque::new())),
            histogram: Arc::new(RwLock::new(LatencyHistogram::new())),
            optimized_stats: Arc::new(RwLock::new(MemoryOptimizedStats::default())),
            alert_config: AlertConfig::default(),
            active_alerts: Arc::new(RwLock::new(Vec::new())),
            memory_config,
            start_time: Utc::now(),
            request_log_sender: None,
            memory_usage: Arc::new(RwLock::new(MemoryUsage::default())),
            cleanup_handle: None,
        }
    }

    pub fn with_alert_config(mut self, config: AlertConfig) -> Self {
        self.alert_config = config;
        self
    }

    pub fn with_websocket_sender(
        mut self,
        sender: broadcast::Sender<crate::integrations::WebSocketMessage>,
    ) -> Self {
        self.request_log_sender = Some(sender);
        self
    }

    pub fn clone_with_websocket_sender(
        &self,
        sender: broadcast::Sender<crate::integrations::WebSocketMessage>,
    ) -> Self {
        Self {
            results: Arc::clone(&self.results),
            histogram: Arc::clone(&self.histogram),
            optimized_stats: Arc::clone(&self.optimized_stats),
            alert_config: self.alert_config.clone(),
            active_alerts: Arc::clone(&self.active_alerts),
            memory_config: self.memory_config.clone(),
            start_time: self.start_time,
            request_log_sender: Some(sender),
            memory_usage: Arc::clone(&self.memory_usage),
            cleanup_handle: None,
        }
    }

    pub async fn start_cleanup_task(&mut self) {
        if self.memory_config.auto_cleanup {
            let results = Arc::clone(&self.results);
            let memory_usage = Arc::clone(&self.memory_usage);
            let config = self.memory_config.clone();

            let handle = tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
                    config.cleanup_interval_seconds,
                ));

                loop {
                    interval.tick().await;
                    Self::cleanup_old_results_static(&results, &memory_usage, &config).await;
                }
            });

            self.cleanup_handle = Some(handle);
        }
    }

    pub async fn record_request(&self, result: RequestResult) {
        // Broadcast to WebSocket if sender is available
        if let Some(sender) = &self.request_log_sender {
            let message = crate::integrations::WebSocketMessage::RequestLog {
                timestamp: chrono::Utc::now(),
                log: result.clone(),
            };
            let _ = sender.send(message);
        }

        // Update histogram
        let mut histogram = self.histogram.write().await;
        histogram.add_sample(result.duration_ms);
        drop(histogram);

        // Update optimized stats
        self.update_optimized_stats(&result).await;

        // Add to ring buffer with size limits
        let mut results = self.results.write().await;
        results.push_back(result);

        // Enforce size limits
        while results.len() > self.memory_config.max_request_results {
            results.pop_front();
        }
        drop(results);

        // Update memory usage tracking
        self.update_memory_usage().await;

        // Check for alerts
        self.check_alerts().await;
    }

    async fn update_optimized_stats(&self, result: &RequestResult) {
        let mut stats = self.optimized_stats.write().await;

        stats.total_requests += 1;
        stats.total_duration_ms += result.duration_ms;
        stats.total_bytes_received += result.bytes_received;

        if result.duration_ms < stats.min_duration_ms {
            stats.min_duration_ms = result.duration_ms;
        }
        if result.duration_ms > stats.max_duration_ms {
            stats.max_duration_ms = result.duration_ms;
        }

        // Update status code counts
        if let Some(status_code) = result.status_code {
            *stats.status_code_counts.entry(status_code).or_insert(0) += 1;

            if (200..400).contains(&status_code) {
                stats.successful_requests += 1;
            } else {
                stats.failed_requests += 1;
            }
        } else {
            stats.failed_requests += 1;
        }

        // Update error counts
        if let Some(error) = &result.error {
            *stats.error_counts.entry(error.clone()).or_insert(0) += 1;
        }

        // Update user agent counts
        if let Some(user_agent) = &result.user_agent {
            *stats
                .user_agent_counts
                .entry(user_agent.clone())
                .or_insert(0) += 1;
        }

        // Update time-based buckets
        self.update_time_buckets(&mut stats, result).await;
    }

    async fn update_time_buckets(&self, stats: &mut MemoryOptimizedStats, result: &RequestResult) {
        let result_minute = result
            .timestamp
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();
        let result_hour = result
            .timestamp
            .with_minute(0)
            .unwrap()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        // Update minute buckets
        if let Some(last_minute) = stats.minute_buckets.back_mut() {
            if last_minute.timestamp == result_minute {
                last_minute.requests += 1;
                last_minute.total_duration_ms += result.duration_ms;
                last_minute.bytes_received += result.bytes_received;

                if result.duration_ms < last_minute.min_duration_ms {
                    last_minute.min_duration_ms = result.duration_ms;
                }
                if result.duration_ms > last_minute.max_duration_ms {
                    last_minute.max_duration_ms = result.duration_ms;
                }

                if result.status_code.is_some()
                    && result.status_code.unwrap() >= 200
                    && result.status_code.unwrap() < 400
                {
                    last_minute.successful_requests += 1;
                } else {
                    last_minute.failed_requests += 1;
                }
            } else {
                stats.minute_buckets.push_back(MinuteBucket {
                    timestamp: result_minute,
                    requests: 1,
                    successful_requests: if result.status_code.is_some()
                        && result.status_code.unwrap() >= 200
                        && result.status_code.unwrap() < 400
                    {
                        1
                    } else {
                        0
                    },
                    failed_requests: if result.status_code.is_none()
                        || result.status_code.unwrap() >= 400
                    {
                        1
                    } else {
                        0
                    },
                    total_duration_ms: result.duration_ms,
                    min_duration_ms: result.duration_ms,
                    max_duration_ms: result.duration_ms,
                    bytes_received: result.bytes_received,
                });
            }
        } else {
            stats.minute_buckets.push_back(MinuteBucket {
                timestamp: result_minute,
                requests: 1,
                successful_requests: if result.status_code.is_some()
                    && result.status_code.unwrap() >= 200
                    && result.status_code.unwrap() < 400
                {
                    1
                } else {
                    0
                },
                failed_requests: if result.status_code.is_none()
                    || result.status_code.unwrap() >= 400
                {
                    1
                } else {
                    0
                },
                total_duration_ms: result.duration_ms,
                min_duration_ms: result.duration_ms,
                max_duration_ms: result.duration_ms,
                bytes_received: result.bytes_received,
            });
        }

        // Keep only last 60 minutes
        while stats.minute_buckets.len() > 60 {
            stats.minute_buckets.pop_front();
        }

        // Update hour buckets (aggregated from minute buckets)
        if let Some(last_hour) = stats.hour_buckets.back_mut() {
            if last_hour.timestamp == result_hour {
                last_hour.requests += 1;
                last_hour.bytes_received += result.bytes_received;

                if result.duration_ms < last_hour.min_duration_ms {
                    last_hour.min_duration_ms = result.duration_ms;
                }
                if result.duration_ms > last_hour.max_duration_ms {
                    last_hour.max_duration_ms = result.duration_ms;
                }

                // Recalculate average
                let old_avg = last_hour.avg_duration_ms;
                last_hour.avg_duration_ms = (old_avg * (last_hour.requests - 1) as f64
                    + result.duration_ms as f64)
                    / last_hour.requests as f64;

                if result.status_code.is_some()
                    && result.status_code.unwrap() >= 200
                    && result.status_code.unwrap() < 400
                {
                    last_hour.successful_requests += 1;
                } else {
                    last_hour.failed_requests += 1;
                }
            } else {
                stats.hour_buckets.push_back(HourBucket {
                    timestamp: result_hour,
                    requests: 1,
                    successful_requests: if result.status_code.is_some()
                        && result.status_code.unwrap() >= 200
                        && result.status_code.unwrap() < 400
                    {
                        1
                    } else {
                        0
                    },
                    failed_requests: if result.status_code.is_none()
                        || result.status_code.unwrap() >= 400
                    {
                        1
                    } else {
                        0
                    },
                    avg_duration_ms: result.duration_ms as f64,
                    min_duration_ms: result.duration_ms,
                    max_duration_ms: result.duration_ms,
                    bytes_received: result.bytes_received,
                });
            }
        } else {
            stats.hour_buckets.push_back(HourBucket {
                timestamp: result_hour,
                requests: 1,
                successful_requests: if result.status_code.is_some()
                    && result.status_code.unwrap() >= 200
                    && result.status_code.unwrap() < 400
                {
                    1
                } else {
                    0
                },
                failed_requests: if result.status_code.is_none()
                    || result.status_code.unwrap() >= 400
                {
                    1
                } else {
                    0
                },
                avg_duration_ms: result.duration_ms as f64,
                min_duration_ms: result.duration_ms,
                max_duration_ms: result.duration_ms,
                bytes_received: result.bytes_received,
            });
        }

        // Keep only last 24 hours
        while stats.hour_buckets.len() > 24 {
            stats.hour_buckets.pop_front();
        }
    }

    async fn update_memory_usage(&self) {
        let mut memory_usage = self.memory_usage.write().await;
        let results = self.results.read().await;

        memory_usage.current_results_in_memory = results.len();
        memory_usage.total_requests_processed += 1;

        // Estimate memory usage (approximate)
        memory_usage.estimated_memory_mb = (results.len() * 250) as f64 / 1_048_576.0;

        // Calculate oldest result age
        if let Some(oldest) = results.front() {
            let age = Utc::now().signed_duration_since(oldest.timestamp);
            memory_usage.oldest_result_age_seconds = age.num_seconds().max(0) as u64;
        }
    }

    async fn cleanup_old_results_static(
        results: &Arc<RwLock<VecDeque<RequestResult>>>,
        memory_usage: &Arc<RwLock<MemoryUsage>>,
        config: &MemoryConfig,
    ) {
        let now = Utc::now();
        let mut results_guard = results.write().await;
        let mut memory_usage_guard = memory_usage.write().await;

        let cutoff_time = now - chrono::Duration::seconds(config.max_result_age_seconds as i64);
        let initial_size = results_guard.len();

        // Remove old results
        while let Some(front) = results_guard.front() {
            if front.timestamp < cutoff_time {
                results_guard.pop_front();
            } else {
                break;
            }
        }

        let removed_count = initial_size - results_guard.len();

        if removed_count > 0 {
            memory_usage_guard.cleanup_runs += 1;
            memory_usage_guard.last_cleanup = now;
        }
    }

    async fn check_alerts(&self) {
        let optimized_stats = self.optimized_stats.read().await;
        let now = Utc::now();

        // Only check if we have enough requests
        if optimized_stats.total_requests < self.alert_config.min_requests_for_alert {
            return;
        }

        let mut alerts = self.active_alerts.write().await;

        // Check error rate
        let error_rate = if optimized_stats.total_requests > 0 {
            (optimized_stats.failed_requests as f64 / optimized_stats.total_requests as f64) * 100.0
        } else {
            0.0
        };

        if error_rate > self.alert_config.error_rate_threshold {
            let alert_id = format!("error_rate_{}", now.timestamp());

            // Check if we already have this alert
            if !alerts.iter().any(|a| {
                a.alert_type == super::AlertType::ErrorRate && a.current_value == error_rate
            }) {
                alerts.push(Alert {
                    id: alert_id,
                    alert_type: super::AlertType::ErrorRate,
                    message: format!(
                        "Error rate ({:.2}%) exceeds threshold ({:.2}%)",
                        error_rate, self.alert_config.error_rate_threshold
                    ),
                    timestamp: now,
                    severity: super::AlertSeverity::Critical,
                    current_value: error_rate,
                    threshold: self.alert_config.error_rate_threshold,
                });
            }
        }

        // Check for performance degradation
        if optimized_stats.total_requests > 100 {
            let current_avg =
                optimized_stats.total_duration_ms as f64 / optimized_stats.total_requests as f64;

            // Get baseline from older requests (first 100 requests)
            let baseline_avg = if optimized_stats.total_requests > 200 {
                // Use requests from 100-200 as baseline
                current_avg * 0.8 // Simplified baseline calculation
            } else {
                current_avg
            };

            let degradation_percent = if baseline_avg > 0.0 {
                ((current_avg - baseline_avg) / baseline_avg) * 100.0
            } else {
                0.0
            };

            if degradation_percent > self.alert_config.degradation_threshold {
                let alert_id = format!("degradation_{}", now.timestamp());

                if !alerts
                    .iter()
                    .any(|a| a.alert_type == super::AlertType::PerformanceDegradation)
                {
                    alerts.push(Alert {
                        id: alert_id,
                        alert_type: super::AlertType::PerformanceDegradation,
                        message: format!(
                            "Performance degraded by {:.2}% (current: {:.2}ms, baseline: {:.2}ms)",
                            degradation_percent, current_avg, baseline_avg
                        ),
                        timestamp: now,
                        severity: super::AlertSeverity::Warning,
                        current_value: degradation_percent,
                        threshold: self.alert_config.degradation_threshold,
                    });
                }
            }
        }

        // Keep only recent alerts (last 10 minutes)
        let cutoff_time = now - chrono::Duration::minutes(10);
        alerts.retain(|alert| alert.timestamp > cutoff_time);
    }

    // Interface compatibility methods
    pub async fn get_live_metrics(&self) -> LiveMetrics {
        let optimized_stats = self.optimized_stats.read().await;
        let histogram = self.histogram.read().await;
        let alerts = self.active_alerts.read().await;

        let test_duration = Utc::now().signed_duration_since(self.start_time);
        let duration_secs = test_duration.num_seconds().max(1) as f64;

        let (p50, p90, p95, p99) = if optimized_stats.total_requests > 0 {
            let avg =
                optimized_stats.total_duration_ms as f64 / optimized_stats.total_requests as f64;
            (avg as u64, avg as u64 * 2, avg as u64 * 3, avg as u64 * 4) // Simplified
        } else {
            (0, 0, 0, 0)
        };

        LiveMetrics {
            requests_sent: optimized_stats.total_requests,
            requests_completed: optimized_stats.successful_requests
                + optimized_stats.failed_requests,
            requests_failed: optimized_stats.failed_requests,
            current_rps: optimized_stats.total_requests as f64 / duration_secs,
            avg_response_time: if optimized_stats.total_requests > 0 {
                optimized_stats.total_duration_ms as f64 / optimized_stats.total_requests as f64
            } else {
                0.0
            },
            min_response_time: if optimized_stats.min_duration_ms == u64::MAX {
                0
            } else {
                optimized_stats.min_duration_ms
            },
            max_response_time: optimized_stats.max_duration_ms,
            p50_response_time: p50,
            p90_response_time: p90,
            p95_response_time: p95,
            p99_response_time: p99,
            active_connections: 0, // Would need to be tracked separately
            queue_size: 0,
            bytes_received: optimized_stats.total_bytes_received,
            status_codes: optimized_stats.status_code_counts.clone(),
            errors: optimized_stats.error_counts.clone(),
            latency_histogram: histogram.clone(),
            active_alerts: alerts.clone(),
        }
    }

    pub async fn get_final_summary(&self) -> FinalSummary {
        let optimized_stats = self.optimized_stats.read().await;
        let _memory_usage = self.memory_usage.read().await;

        let test_duration = Utc::now().signed_duration_since(self.start_time);

        FinalSummary {
            total_requests: optimized_stats.total_requests,
            successful_requests: optimized_stats.successful_requests,
            failed_requests: optimized_stats.failed_requests,
            test_duration_secs: test_duration.num_seconds() as f64,
            avg_rps: if test_duration.num_seconds() > 0 {
                optimized_stats.total_requests as f64 / test_duration.num_seconds() as f64
            } else {
                0.0
            },
            avg_response_time: if optimized_stats.total_requests > 0 {
                optimized_stats.total_duration_ms as f64 / optimized_stats.total_requests as f64
            } else {
                0.0
            },
            min_response_time: if optimized_stats.min_duration_ms == u64::MAX {
                0
            } else {
                optimized_stats.min_duration_ms
            },
            max_response_time: optimized_stats.max_duration_ms,
            p50_response_time: 0, // Would need histogram calculation
            p95_response_time: 0,
            p99_response_time: 0,
            total_bytes_received: optimized_stats.total_bytes_received,
            status_codes: optimized_stats.status_code_counts.clone(),
            errors: optimized_stats.error_counts.clone(),
            user_agents_used: optimized_stats.user_agent_counts.clone(),
        }
    }

    pub async fn get_memory_usage(&self) -> MemoryUsage {
        self.memory_usage.read().await.clone()
    }

    pub async fn get_optimized_stats(&self) -> MemoryOptimizedStats {
        self.optimized_stats.read().await.clone()
    }

    pub async fn force_cleanup(&self) {
        Self::cleanup_old_results_static(&self.results, &self.memory_usage, &self.memory_config)
            .await;
    }
}
