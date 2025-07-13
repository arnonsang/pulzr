use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestResult {
    pub timestamp: DateTime<Utc>,
    pub duration_ms: u64,
    pub status_code: Option<u16>,
    pub error: Option<String>,
    pub user_agent: Option<String>,
    pub bytes_received: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub error_rate_threshold: f64,    // Percentage (0.0-100.0)
    pub min_requests_for_alert: u64,  // Minimum requests before alerting
    pub alert_window_seconds: u64,    // Time window for calculating rates
    pub degradation_threshold: f64, // Percentage increase for degradation (e.g., 50.0 for 50% increase)
    pub baseline_window_seconds: u64, // Time window for baseline calculation
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            error_rate_threshold: 10.0,   // 10% error rate
            min_requests_for_alert: 10,   // At least 10 requests
            alert_window_seconds: 60,     // 1 minute window
            degradation_threshold: 50.0,  // 50% increase in response time
            baseline_window_seconds: 300, // 5 minute baseline window
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub alert_type: AlertType,
    pub message: String,
    pub timestamp: DateTime<Utc>,
    pub severity: AlertSeverity,
    pub current_value: f64,
    pub threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AlertType {
    ErrorRate,
    ResponseTime,
    PerformanceDegradation,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatencyHistogram {
    pub buckets: Vec<u64>,
    pub bucket_bounds: Vec<u64>,
    pub max_value: u64,
    pub total_count: u64,
}

impl Default for LatencyHistogram {
    fn default() -> Self {
        Self::new()
    }
}

impl LatencyHistogram {
    pub fn new() -> Self {
        // Define histogram buckets: 0-10ms, 10-50ms, 50-100ms, 100-250ms, 250-500ms, 500-1000ms, 1000-2000ms, 2000-5000ms, 5000+ms
        let bucket_bounds = vec![10, 50, 100, 250, 500, 1000, 2000, 5000, u64::MAX];
        let buckets = vec![0; bucket_bounds.len()];

        Self {
            buckets,
            bucket_bounds,
            max_value: 0,
            total_count: 0,
        }
    }

    pub fn add_sample(&mut self, value: u64) {
        self.total_count += 1;
        self.max_value = self.max_value.max(value);

        // Find the appropriate bucket
        for (i, &bound) in self.bucket_bounds.iter().enumerate() {
            if value <= bound {
                self.buckets[i] += 1;
                break;
            }
        }
    }

    pub fn get_bucket_labels(&self) -> Vec<String> {
        let mut labels = Vec::new();
        let mut prev_bound = 0;

        for &bound in &self.bucket_bounds {
            if bound == u64::MAX {
                labels.push(format!("{}ms+", prev_bound));
            } else {
                labels.push(format!("{}-{}ms", prev_bound, bound));
            }
            prev_bound = bound;
        }

        labels
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiveMetrics {
    pub requests_sent: u64,
    pub requests_completed: u64,
    pub requests_failed: u64,
    pub current_rps: f64,
    pub avg_response_time: f64,
    pub min_response_time: u64,
    pub max_response_time: u64,
    pub p50_response_time: u64,
    pub p90_response_time: u64,
    pub p95_response_time: u64,
    pub p99_response_time: u64,
    pub active_connections: u64,
    pub queue_size: u64,
    pub bytes_received: u64,
    pub status_codes: HashMap<u16, u64>,
    pub errors: HashMap<String, u64>,
    pub latency_histogram: LatencyHistogram,
    pub active_alerts: Vec<Alert>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalSummary {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub test_duration_secs: f64,
    pub avg_rps: f64,
    pub avg_response_time: f64,
    pub min_response_time: u64,
    pub max_response_time: u64,
    pub p50_response_time: u64,
    pub p95_response_time: u64,
    pub p99_response_time: u64,
    pub total_bytes_received: u64,
    pub status_codes: HashMap<u16, u64>,
    pub errors: HashMap<String, u64>,
    pub user_agents_used: HashMap<String, u64>,
}

pub struct StatsCollector {
    pub results: Arc<RwLock<Vec<RequestResult>>>,
    histogram: Arc<RwLock<LatencyHistogram>>,
    alert_config: AlertConfig,
    active_alerts: Arc<RwLock<Vec<Alert>>>,
    start_time: DateTime<Utc>,
    request_log_sender: Option<broadcast::Sender<crate::websocket::WebSocketMessage>>,
}

impl Default for StatsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsCollector {
    pub fn new() -> Self {
        Self {
            results: Arc::new(RwLock::new(Vec::new())),
            histogram: Arc::new(RwLock::new(LatencyHistogram::new())),
            alert_config: AlertConfig::default(),
            active_alerts: Arc::new(RwLock::new(Vec::new())),
            start_time: Utc::now(),
            request_log_sender: None,
        }
    }

    pub fn with_alert_config(mut self, config: AlertConfig) -> Self {
        self.alert_config = config;
        self
    }

    fn calculate_percentiles(sorted_times: &[u64]) -> (u64, u64, u64, u64) {
        if sorted_times.is_empty() {
            return (0, 0, 0, 0);
        }

        let len = sorted_times.len();
        let p50_index = (len * 50) / 100;
        let p90_index = (len * 90) / 100;
        let p95_index = (len * 95) / 100;
        let p99_index = (len * 99) / 100;

        let p50 = sorted_times[p50_index.saturating_sub(1)];
        let p90 = sorted_times[p90_index.saturating_sub(1)];
        let p95 = sorted_times[p95_index.saturating_sub(1)];
        let p99 = sorted_times[p99_index.saturating_sub(1)];

        (p50, p90, p95, p99)
    }

    pub fn with_websocket_sender(
        mut self,
        sender: broadcast::Sender<crate::websocket::WebSocketMessage>,
    ) -> Self {
        self.request_log_sender = Some(sender);
        self
    }

    pub fn clone_with_websocket_sender(
        &self,
        sender: broadcast::Sender<crate::websocket::WebSocketMessage>,
    ) -> Self {
        Self {
            results: Arc::clone(&self.results),
            histogram: Arc::clone(&self.histogram),
            alert_config: self.alert_config.clone(),
            active_alerts: Arc::clone(&self.active_alerts),
            start_time: self.start_time,
            request_log_sender: Some(sender),
        }
    }

    pub async fn record_request(&self, result: RequestResult) {
        // Broadcast to WebSocket if sender is available
        if let Some(sender) = &self.request_log_sender {
            let message = crate::websocket::WebSocketMessage::RequestLog {
                timestamp: chrono::Utc::now(),
                log: result.clone(),
            };
            let _ = sender.send(message); // Ignore if no receivers
        }

        // Update histogram with response time
        let mut histogram = self.histogram.write().await;
        histogram.add_sample(result.duration_ms);
        drop(histogram);

        let mut results = self.results.write().await;
        results.push(result);
        drop(results);

        // Check for alerts
        self.check_alerts().await;
    }

    async fn check_alerts(&self) {
        let results = self.results.read().await;
        let now = Utc::now();

        // Only check if we have enough requests
        if results.len() < self.alert_config.min_requests_for_alert as usize {
            return;
        }

        // Calculate recent error rate
        let window_start =
            now - chrono::Duration::seconds(self.alert_config.alert_window_seconds as i64);
        let recent_results: Vec<_> = results
            .iter()
            .filter(|r| r.timestamp > window_start)
            .collect();

        if recent_results.is_empty() {
            return;
        }

        let recent_total = recent_results.len() as u64;
        let recent_errors = recent_results.iter().filter(|r| r.error.is_some()).count() as u64;

        let error_rate = (recent_errors as f64 / recent_total as f64) * 100.0;

        // Check if error rate exceeds threshold
        if error_rate > self.alert_config.error_rate_threshold {
            let alert = Alert {
                id: format!("error_rate_{}", now.timestamp()),
                alert_type: AlertType::ErrorRate,
                message: format!(
                    "Error rate {}% exceeds threshold of {}% ({} errors out of {} requests in last {} seconds)",
                    error_rate.round() as u64,
                    self.alert_config.error_rate_threshold.round() as u64,
                    recent_errors,
                    recent_total,
                    self.alert_config.alert_window_seconds
                ),
                timestamp: now,
                severity: if error_rate > 50.0 { AlertSeverity::Critical } else { AlertSeverity::Warning },
                current_value: error_rate,
                threshold: self.alert_config.error_rate_threshold,
            };

            // Check if we already have this alert active
            let mut active_alerts = self.active_alerts.write().await;
            if !active_alerts
                .iter()
                .any(|a| matches!(a.alert_type, AlertType::ErrorRate))
            {
                active_alerts.push(alert.clone());

                // Send alert via WebSocket if available
                if let Some(sender) = &self.request_log_sender {
                    let message = crate::websocket::WebSocketMessage::ErrorEvent {
                        timestamp: now,
                        error: alert.message.clone(),
                    };
                    let _ = sender.send(message);
                }

                // Print alert to console
                println!("⚠️  ALERT: {}", alert.message);
            }
        } else {
            // Clear error rate alerts if error rate is back to normal
            let mut active_alerts = self.active_alerts.write().await;
            active_alerts.retain(|a| !matches!(a.alert_type, AlertType::ErrorRate));
        }

        // Check for performance degradation
        self.check_performance_degradation(&results, now).await;
    }

    async fn check_performance_degradation(&self, results: &[RequestResult], now: DateTime<Utc>) {
        if results.len() < (self.alert_config.min_requests_for_alert * 2) as usize {
            return;
        }

        // Calculate baseline (older period)
        let baseline_start =
            now - chrono::Duration::seconds(self.alert_config.baseline_window_seconds as i64);
        let baseline_end =
            now - chrono::Duration::seconds(self.alert_config.alert_window_seconds as i64);

        let baseline_results: Vec<_> = results
            .iter()
            .filter(|r| r.timestamp >= baseline_start && r.timestamp <= baseline_end)
            .filter(|r| r.error.is_none()) // Only consider successful requests
            .collect();

        // Calculate recent performance (recent period)
        let recent_start =
            now - chrono::Duration::seconds(self.alert_config.alert_window_seconds as i64);
        let recent_results: Vec<_> = results
            .iter()
            .filter(|r| r.timestamp >= recent_start)
            .filter(|r| r.error.is_none()) // Only consider successful requests
            .collect();

        // Need enough data in both periods
        if baseline_results.len() < 5 || recent_results.len() < 5 {
            return;
        }

        // Calculate average response times
        let baseline_avg = baseline_results.iter().map(|r| r.duration_ms).sum::<u64>() as f64
            / baseline_results.len() as f64;

        let recent_avg = recent_results.iter().map(|r| r.duration_ms).sum::<u64>() as f64
            / recent_results.len() as f64;

        // Calculate percentage increase
        let increase_percentage = ((recent_avg - baseline_avg) / baseline_avg) * 100.0;

        // Check if degradation exceeds threshold
        if increase_percentage > self.alert_config.degradation_threshold {
            let alert = Alert {
                id: format!("degradation_{}", now.timestamp()),
                alert_type: AlertType::PerformanceDegradation,
                message: format!(
                    "Performance degradation detected: response time increased by {:.1}% (from {:.0}ms to {:.0}ms)",
                    increase_percentage,
                    baseline_avg,
                    recent_avg
                ),
                timestamp: now,
                severity: if increase_percentage > 100.0 { AlertSeverity::Critical } else { AlertSeverity::Warning },
                current_value: increase_percentage,
                threshold: self.alert_config.degradation_threshold,
            };

            // Check if we already have this alert active
            let mut active_alerts = self.active_alerts.write().await;
            if !active_alerts
                .iter()
                .any(|a| matches!(a.alert_type, AlertType::PerformanceDegradation))
            {
                active_alerts.push(alert.clone());

                // Send alert via WebSocket if available
                if let Some(sender) = &self.request_log_sender {
                    let message = crate::websocket::WebSocketMessage::ErrorEvent {
                        timestamp: now,
                        error: format!("DEGRADATION: {}", alert.message),
                    };
                    let _ = sender.send(message);
                }

                // Print alert to console
                println!("📉 DEGRADATION: {}", alert.message);
            }
        } else {
            // Clear degradation alerts if performance is back to normal
            let mut active_alerts = self.active_alerts.write().await;
            active_alerts.retain(|a| !matches!(a.alert_type, AlertType::PerformanceDegradation));
        }
    }

    pub async fn get_live_metrics(&self) -> LiveMetrics {
        let results = self.results.read().await;
        let histogram = self.histogram.read().await;
        let active_alerts = self.active_alerts.read().await;
        let now = Utc::now();
        let test_duration = (now - self.start_time).num_seconds() as f64;

        let total_requests = results.len() as u64;
        let successful_requests = results.iter().filter(|r| r.error.is_none()).count() as u64;
        let failed_requests = total_requests - successful_requests;

        let mut status_codes = HashMap::new();
        let mut errors = HashMap::new();
        let mut response_times = Vec::new();
        let mut total_bytes = 0u64;

        for result in results.iter() {
            if let Some(code) = result.status_code {
                *status_codes.entry(code).or_insert(0) += 1;
            }

            if let Some(error) = &result.error {
                *errors.entry(error.clone()).or_insert(0) += 1;
            }

            response_times.push(result.duration_ms);
            total_bytes += result.bytes_received;
        }

        let (avg_response_time, min_response_time, max_response_time, p50, p90, p95, p99) =
            if response_times.is_empty() {
                (0.0, 0, 0, 0, 0, 0, 0)
            } else {
                let avg = response_times.iter().sum::<u64>() as f64 / response_times.len() as f64;
                let min = *response_times.iter().min().unwrap_or(&0);
                let max = *response_times.iter().max().unwrap_or(&0);

                // Sort for percentile calculation
                response_times.sort_unstable();
                let (p50, p90, p95, p99) = Self::calculate_percentiles(&response_times);

                (avg, min, max, p50, p90, p95, p99)
            };

        let current_rps = if test_duration > 0.0 {
            total_requests as f64 / test_duration
        } else {
            0.0
        };

        LiveMetrics {
            requests_sent: total_requests,
            requests_completed: total_requests,
            requests_failed: failed_requests,
            current_rps,
            avg_response_time,
            min_response_time,
            max_response_time,
            p50_response_time: p50,
            p90_response_time: p90,
            p95_response_time: p95,
            p99_response_time: p99,
            active_connections: 0,
            queue_size: 0,
            bytes_received: total_bytes,
            status_codes,
            errors,
            latency_histogram: histogram.clone(),
            active_alerts: active_alerts.clone(),
        }
    }

    pub async fn get_final_summary(&self) -> FinalSummary {
        let results = self.results.read().await;
        let end_time = Utc::now();
        let test_duration = (end_time - self.start_time).num_seconds() as f64;

        let total_requests = results.len() as u64;
        let successful_requests = results.iter().filter(|r| r.error.is_none()).count() as u64;
        let failed_requests = total_requests - successful_requests;

        let mut status_codes = HashMap::new();
        let mut errors = HashMap::new();
        let mut user_agents = HashMap::new();
        let mut response_times = Vec::new();
        let mut total_bytes = 0u64;

        for result in results.iter() {
            if let Some(code) = result.status_code {
                *status_codes.entry(code).or_insert(0) += 1;
            }

            if let Some(error) = &result.error {
                *errors.entry(error.clone()).or_insert(0) += 1;
            }

            if let Some(ua) = &result.user_agent {
                *user_agents.entry(ua.clone()).or_insert(0) += 1;
            }

            response_times.push(result.duration_ms);
            total_bytes += result.bytes_received;
        }

        response_times.sort_unstable();

        let (avg_response_time, min_response_time, max_response_time, p50, p95, p99) =
            if response_times.is_empty() {
                (0.0, 0, 0, 0, 0, 0)
            } else {
                let avg = response_times.iter().sum::<u64>() as f64 / response_times.len() as f64;
                let min = response_times[0];
                let max = response_times[response_times.len() - 1];
                let p50 = response_times[response_times.len() * 50 / 100];
                let p95 = response_times[response_times.len() * 95 / 100];
                let p99 = response_times[response_times.len() * 99 / 100];
                (avg, min, max, p50, p95, p99)
            };

        let avg_rps = if test_duration > 0.0 {
            total_requests as f64 / test_duration
        } else {
            0.0
        };

        FinalSummary {
            total_requests,
            successful_requests,
            failed_requests,
            test_duration_secs: test_duration,
            avg_rps,
            avg_response_time,
            min_response_time,
            max_response_time,
            p50_response_time: p50,
            p95_response_time: p95,
            p99_response_time: p99,
            total_bytes_received: total_bytes,
            status_codes,
            errors,
            user_agents_used: user_agents,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn test_alert_config_default() {
        let config = AlertConfig::default();

        assert_eq!(config.error_rate_threshold, 10.0);
        assert_eq!(config.min_requests_for_alert, 10);
        assert_eq!(config.alert_window_seconds, 60);
        assert_eq!(config.degradation_threshold, 50.0);
        assert_eq!(config.baseline_window_seconds, 300);
    }

    #[test]
    fn test_alert_creation() {
        let alert = Alert {
            id: "test-alert-1".to_string(),
            alert_type: AlertType::ErrorRate,
            message: "High error rate detected".to_string(),
            timestamp: Utc::now(),
            severity: AlertSeverity::Warning,
            current_value: 15.0,
            threshold: 10.0,
        };

        assert_eq!(alert.id, "test-alert-1");
        assert_eq!(alert.alert_type, AlertType::ErrorRate);
        assert!(alert.current_value > alert.threshold);
    }

    #[test]
    fn test_alert_type_equality() {
        assert_eq!(AlertType::ErrorRate, AlertType::ErrorRate);
        assert_eq!(AlertType::ResponseTime, AlertType::ResponseTime);
        assert_eq!(
            AlertType::Custom("test".to_string()),
            AlertType::Custom("test".to_string())
        );
        assert_ne!(AlertType::ErrorRate, AlertType::ResponseTime);
    }

    #[test]
    fn test_latency_histogram_creation() {
        let histogram = LatencyHistogram::new();

        assert_eq!(histogram.buckets.len(), 9);
        assert_eq!(histogram.bucket_bounds.len(), 9);
        assert_eq!(histogram.total_count, 0);
        assert_eq!(histogram.max_value, 0);

        // Check bucket bounds
        assert_eq!(histogram.bucket_bounds[0], 10);
        assert_eq!(histogram.bucket_bounds[1], 50);
        assert_eq!(histogram.bucket_bounds[8], u64::MAX);
    }

    #[test]
    fn test_latency_histogram_add_sample() {
        let mut histogram = LatencyHistogram::new();

        // Add samples to different buckets
        histogram.add_sample(5); // Bucket 0 (0-10ms)
        histogram.add_sample(25); // Bucket 1 (10-50ms)
        histogram.add_sample(75); // Bucket 2 (50-100ms)
        histogram.add_sample(200); // Bucket 3 (100-250ms)
        histogram.add_sample(1500); // Bucket 6 (1000-2000ms)

        assert_eq!(histogram.total_count, 5);
        assert_eq!(histogram.max_value, 1500);
        assert_eq!(histogram.buckets[0], 1); // 5ms sample
        assert_eq!(histogram.buckets[1], 1); // 25ms sample
        assert_eq!(histogram.buckets[2], 1); // 75ms sample
        assert_eq!(histogram.buckets[3], 1); // 200ms sample
        assert_eq!(histogram.buckets[6], 1); // 1500ms sample
    }

    #[test]
    fn test_latency_histogram_bucket_labels() {
        let histogram = LatencyHistogram::new();
        let labels = histogram.get_bucket_labels();

        assert_eq!(labels.len(), 9);
        assert_eq!(labels[0], "0-10ms");
        assert_eq!(labels[1], "10-50ms");
        assert_eq!(labels[8], "5000ms+");
    }

    #[test]
    fn test_latency_histogram_percentiles() {
        // Test the static calculate_percentiles method directly
        let mut response_times = vec![5, 15, 25, 35, 45, 55, 65, 75, 85, 95];
        response_times.sort();

        let (p50, p90, p95, p99) = StatsCollector::calculate_percentiles(&response_times);

        // With sorted data, verify percentiles are reasonable
        assert!(p50 > 0);
        assert!(p90 >= p50);
        assert!(p95 >= p90);
        assert!(p99 >= p95);

        // Specific value checks
        assert_eq!(p50, 45); // 50th percentile
        assert_eq!(p95, 85); // 95th percentile
    }

    #[test]
    fn test_request_result_creation() {
        let result = RequestResult {
            timestamp: Utc::now(),
            duration_ms: 150,
            status_code: Some(200),
            error: None,
            user_agent: Some("test-agent".to_string()),
            bytes_received: 1024,
        };

        assert_eq!(result.duration_ms, 150);
        assert_eq!(result.status_code, Some(200));
        assert_eq!(result.error, None);
        assert_eq!(result.bytes_received, 1024);
    }

    #[test]
    fn test_request_result_with_error() {
        let result = RequestResult {
            timestamp: Utc::now(),
            duration_ms: 5000,
            status_code: None,
            error: Some("Connection timeout".to_string()),
            user_agent: None,
            bytes_received: 0,
        };

        assert_eq!(result.status_code, None);
        assert_eq!(result.error, Some("Connection timeout".to_string()));
        assert_eq!(result.bytes_received, 0);
    }

    #[test]
    fn test_alert_severity_serialization() {
        // Test that alert severity can be serialized/deserialized
        let severities = vec![
            AlertSeverity::Info,
            AlertSeverity::Warning,
            AlertSeverity::Critical,
        ];

        for severity in severities {
            let json = serde_json::to_string(&severity).unwrap();
            let _deserialized: AlertSeverity = serde_json::from_str(&json).unwrap();

            // Can't directly compare due to no PartialEq, but serialization should work
            assert!(!json.is_empty());
        }
    }

    #[test]
    fn test_histogram_edge_cases() {
        let mut histogram = LatencyHistogram::new();

        // Test extreme values
        histogram.add_sample(0);
        histogram.add_sample(u64::MAX);

        assert_eq!(histogram.total_count, 2);
        assert_eq!(histogram.max_value, u64::MAX);
        assert_eq!(histogram.buckets[0], 1); // 0 goes to first bucket
        assert_eq!(histogram.buckets[8], 1); // u64::MAX goes to last bucket
    }
}
