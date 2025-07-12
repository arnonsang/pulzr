use crate::metrics::RequestResult;
use chrono::{DateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum number of individual request results to keep in memory
    pub max_request_results: usize,
    /// Maximum age of request results to keep (in seconds)
    pub max_result_age_seconds: u64,
    /// Enable streaming mode for large tests
    pub enable_streaming: bool,
    /// Batch size for processing aggregated metrics
    pub batch_size: usize,
    /// Enable periodic memory cleanup
    pub auto_cleanup: bool,
    /// Cleanup interval in seconds
    pub cleanup_interval_seconds: u64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_request_results: 10_000,  // Keep last 10k results
            max_result_age_seconds: 3600, // Keep results for 1 hour
            enable_streaming: false,      // Disabled by default
            batch_size: 1000,             // Process in 1k batches
            auto_cleanup: true,           // Enable automatic cleanup
            cleanup_interval_seconds: 60, // Cleanup every minute
        }
    }
}

impl MemoryConfig {
    pub fn streaming() -> Self {
        Self {
            max_request_results: 1_000,  // Keep fewer results in streaming mode
            max_result_age_seconds: 300, // Keep results for 5 minutes
            enable_streaming: true,
            batch_size: 100, // Smaller batches for streaming
            auto_cleanup: true,
            cleanup_interval_seconds: 30, // More frequent cleanup
        }
    }

    pub fn high_throughput() -> Self {
        Self {
            max_request_results: 50_000,  // Keep more results for high throughput
            max_result_age_seconds: 7200, // Keep results for 2 hours
            enable_streaming: false,
            batch_size: 5000, // Larger batches
            auto_cleanup: true,
            cleanup_interval_seconds: 120, // Less frequent cleanup
        }
    }

    pub fn low_memory() -> Self {
        Self {
            max_request_results: 500,   // Keep very few results
            max_result_age_seconds: 60, // Keep results for 1 minute
            enable_streaming: true,
            batch_size: 50, // Small batches
            auto_cleanup: true,
            cleanup_interval_seconds: 10, // Very frequent cleanup
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    pub total_requests_processed: u64,
    pub current_requests_in_memory: usize,
    pub memory_usage_estimate_mb: f64,
    pub oldest_request_age_seconds: u64,
    pub cleanup_runs: u64,
    pub last_cleanup: DateTime<Utc>,
    pub streaming_enabled: bool,
    pub batches_processed: u64,
}

impl Default for MemoryStats {
    fn default() -> Self {
        Self {
            total_requests_processed: 0,
            current_requests_in_memory: 0,
            memory_usage_estimate_mb: 0.0,
            oldest_request_age_seconds: 0,
            cleanup_runs: 0,
            last_cleanup: Utc::now(),
            streaming_enabled: false,
            batches_processed: 0,
        }
    }
}

pub struct StreamingStatsCollector {
    /// Ring buffer for recent request results
    results: Arc<RwLock<VecDeque<RequestResult>>>,
    /// Aggregated statistics (never cleared)
    aggregated_stats: Arc<RwLock<AggregatedStats>>,
    /// Memory configuration
    config: MemoryConfig,
    /// Memory usage statistics
    memory_stats: Arc<RwLock<MemoryStats>>,
    /// Start time for age calculations
    start_time: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregatedStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub total_duration_ms: u64,
    pub min_duration_ms: u64,
    pub max_duration_ms: u64,
    pub total_bytes_received: u64,
    pub status_code_counts: std::collections::HashMap<u16, u64>,
    pub error_counts: std::collections::HashMap<String, u64>,
    pub hourly_stats: VecDeque<HourlyStats>,
    pub minute_stats: VecDeque<MinuteStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyStats {
    pub hour: DateTime<Utc>,
    pub requests: u64,
    pub avg_duration_ms: f64,
    pub error_rate: f64,
    pub throughput_rps: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinuteStats {
    pub minute: DateTime<Utc>,
    pub requests: u64,
    pub avg_duration_ms: f64,
    pub error_rate: f64,
    pub throughput_rps: f64,
}

impl Default for AggregatedStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            total_duration_ms: 0,
            min_duration_ms: u64::MAX,
            max_duration_ms: 0,
            total_bytes_received: 0,
            status_code_counts: std::collections::HashMap::new(),
            error_counts: std::collections::HashMap::new(),
            hourly_stats: VecDeque::new(),
            minute_stats: VecDeque::new(),
        }
    }
}

impl StreamingStatsCollector {
    pub fn new(config: MemoryConfig) -> Self {
        Self {
            results: Arc::new(RwLock::new(VecDeque::new())),
            aggregated_stats: Arc::new(RwLock::new(AggregatedStats::default())),
            config,
            memory_stats: Arc::new(RwLock::new(MemoryStats::default())),
            start_time: Utc::now(),
        }
    }

    pub async fn record_request(&self, result: RequestResult) {
        // Update aggregated stats (always kept)
        self.update_aggregated_stats(&result).await;

        // Add to ring buffer (with size limits)
        let mut results = self.results.write().await;
        results.push_back(result);

        // Enforce size limits
        while results.len() > self.config.max_request_results {
            results.pop_front();
        }

        drop(results);

        // Update memory stats
        self.update_memory_stats().await;

        // Perform cleanup if needed
        if self.config.auto_cleanup {
            self.cleanup_old_results().await;
        }
    }

    async fn update_aggregated_stats(&self, result: &RequestResult) {
        let mut stats = self.aggregated_stats.write().await;

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

        // Update time-based stats
        self.update_time_based_stats(&mut stats, result).await;
    }

    async fn update_time_based_stats(&self, stats: &mut AggregatedStats, result: &RequestResult) {
        let result_minute = result
            .timestamp
            .date_naive()
            .and_hms_opt(result.timestamp.hour(), result.timestamp.minute(), 0)
            .unwrap()
            .and_utc();

        let result_hour = result
            .timestamp
            .date_naive()
            .and_hms_opt(result.timestamp.hour(), 0, 0)
            .unwrap()
            .and_utc();

        // Update minute stats
        if let Some(last_minute) = stats.minute_stats.back_mut() {
            if last_minute.minute == result_minute {
                last_minute.requests += 1;
                last_minute.avg_duration_ms = (last_minute.avg_duration_ms
                    * (last_minute.requests - 1) as f64
                    + result.duration_ms as f64)
                    / last_minute.requests as f64;
                last_minute.error_rate =
                    if result.status_code.is_some() && result.status_code.unwrap() >= 400 {
                        (last_minute.error_rate * (last_minute.requests - 1) as f64 + 1.0)
                            / last_minute.requests as f64
                    } else {
                        (last_minute.error_rate * (last_minute.requests - 1) as f64)
                            / last_minute.requests as f64
                    };
            } else {
                stats.minute_stats.push_back(MinuteStats {
                    minute: result_minute,
                    requests: 1,
                    avg_duration_ms: result.duration_ms as f64,
                    error_rate: if result.status_code.is_some()
                        && result.status_code.unwrap() >= 400
                    {
                        1.0
                    } else {
                        0.0
                    },
                    throughput_rps: 1.0 / 60.0,
                });
            }
        } else {
            stats.minute_stats.push_back(MinuteStats {
                minute: result_minute,
                requests: 1,
                avg_duration_ms: result.duration_ms as f64,
                error_rate: if result.status_code.is_some() && result.status_code.unwrap() >= 400 {
                    1.0
                } else {
                    0.0
                },
                throughput_rps: 1.0 / 60.0,
            });
        }

        // Keep only last 60 minutes
        while stats.minute_stats.len() > 60 {
            stats.minute_stats.pop_front();
        }

        // Update hour stats
        if let Some(last_hour) = stats.hourly_stats.back_mut() {
            if last_hour.hour == result_hour {
                last_hour.requests += 1;
                last_hour.avg_duration_ms = (last_hour.avg_duration_ms
                    * (last_hour.requests - 1) as f64
                    + result.duration_ms as f64)
                    / last_hour.requests as f64;
                last_hour.error_rate =
                    if result.status_code.is_some() && result.status_code.unwrap() >= 400 {
                        (last_hour.error_rate * (last_hour.requests - 1) as f64 + 1.0)
                            / last_hour.requests as f64
                    } else {
                        (last_hour.error_rate * (last_hour.requests - 1) as f64)
                            / last_hour.requests as f64
                    };
            } else {
                stats.hourly_stats.push_back(HourlyStats {
                    hour: result_hour,
                    requests: 1,
                    avg_duration_ms: result.duration_ms as f64,
                    error_rate: if result.status_code.is_some()
                        && result.status_code.unwrap() >= 400
                    {
                        1.0
                    } else {
                        0.0
                    },
                    throughput_rps: 1.0 / 3600.0,
                });
            }
        } else {
            stats.hourly_stats.push_back(HourlyStats {
                hour: result_hour,
                requests: 1,
                avg_duration_ms: result.duration_ms as f64,
                error_rate: if result.status_code.is_some() && result.status_code.unwrap() >= 400 {
                    1.0
                } else {
                    0.0
                },
                throughput_rps: 1.0 / 3600.0,
            });
        }

        // Keep only last 24 hours
        while stats.hourly_stats.len() > 24 {
            stats.hourly_stats.pop_front();
        }
    }

    async fn update_memory_stats(&self) {
        let mut memory_stats = self.memory_stats.write().await;
        let results = self.results.read().await;

        memory_stats.current_requests_in_memory = results.len();
        memory_stats.total_requests_processed += 1;

        // Estimate memory usage (rough calculation)
        // Each RequestResult is approximately 200 bytes
        memory_stats.memory_usage_estimate_mb = (results.len() * 200) as f64 / 1_048_576.0;

        // Calculate oldest request age
        if let Some(oldest) = results.front() {
            let age = Utc::now().signed_duration_since(oldest.timestamp);
            memory_stats.oldest_request_age_seconds = age.num_seconds() as u64;
        }

        memory_stats.streaming_enabled = self.config.enable_streaming;
    }

    async fn cleanup_old_results(&self) {
        let now = Utc::now();
        let mut results = self.results.write().await;
        let mut memory_stats = self.memory_stats.write().await;

        let cutoff_time =
            now - chrono::Duration::seconds(self.config.max_result_age_seconds as i64);

        let initial_size = results.len();

        // Remove old results
        while let Some(front) = results.front() {
            if front.timestamp < cutoff_time {
                results.pop_front();
            } else {
                break;
            }
        }

        let removed_count = initial_size - results.len();

        if removed_count > 0 {
            memory_stats.cleanup_runs += 1;
            memory_stats.last_cleanup = now;
        }
    }

    pub async fn get_memory_stats(&self) -> MemoryStats {
        self.memory_stats.read().await.clone()
    }

    pub async fn get_aggregated_stats(&self) -> AggregatedStats {
        self.aggregated_stats.read().await.clone()
    }

    pub async fn get_recent_results(&self, count: usize) -> Vec<RequestResult> {
        let results = self.results.read().await;
        results.iter().rev().take(count).cloned().collect()
    }

    pub async fn get_results_in_time_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Vec<RequestResult> {
        let results = self.results.read().await;
        results
            .iter()
            .filter(|r| r.timestamp >= start && r.timestamp <= end)
            .cloned()
            .collect()
    }

    pub async fn force_cleanup(&self) {
        self.cleanup_old_results().await;
    }

    pub async fn get_streaming_summary(&self) -> StreamingSummary {
        let memory_stats = self.memory_stats.read().await;
        let aggregated_stats = self.aggregated_stats.read().await;

        StreamingSummary {
            memory_stats: memory_stats.clone(),
            total_requests: aggregated_stats.total_requests,
            success_rate: if aggregated_stats.total_requests > 0 {
                (aggregated_stats.successful_requests as f64
                    / aggregated_stats.total_requests as f64)
                    * 100.0
            } else {
                0.0
            },
            avg_response_time: if aggregated_stats.total_requests > 0 {
                aggregated_stats.total_duration_ms as f64 / aggregated_stats.total_requests as f64
            } else {
                0.0
            },
            throughput_rps: if aggregated_stats.total_requests > 0 {
                let test_duration = Utc::now().signed_duration_since(self.start_time);
                aggregated_stats.total_requests as f64 / test_duration.num_seconds() as f64
            } else {
                0.0
            },
            total_bytes_received: aggregated_stats.total_bytes_received,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingSummary {
    pub memory_stats: MemoryStats,
    pub total_requests: u64,
    pub success_rate: f64,
    pub avg_response_time: f64,
    pub throughput_rps: f64,
    pub total_bytes_received: u64,
}

impl StreamingSummary {
    pub fn print_summary(&self) {
        println!("🔄 Streaming Stats Summary:");
        println!("   Total Requests: {}", self.total_requests);
        println!("   Success Rate: {:.2}%", self.success_rate);
        println!("   Avg Response Time: {:.2}ms", self.avg_response_time);
        println!("   Throughput: {:.2} RPS", self.throughput_rps);
        println!("   Total Bytes: {}", self.total_bytes_received);
        println!(
            "   Memory Usage: {:.2} MB",
            self.memory_stats.memory_usage_estimate_mb
        );
        println!(
            "   Requests in Memory: {}",
            self.memory_stats.current_requests_in_memory
        );
        println!("   Cleanup Runs: {}", self.memory_stats.cleanup_runs);
        println!(
            "   Oldest Request Age: {}s",
            self.memory_stats.oldest_request_age_seconds
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn test_memory_config_defaults() {
        let config = MemoryConfig::default();
        assert_eq!(config.max_request_results, 10_000);
        assert_eq!(config.max_result_age_seconds, 3600);
        assert!(!config.enable_streaming);
    }

    #[tokio::test]
    async fn test_streaming_config() {
        let config = MemoryConfig::streaming();
        assert_eq!(config.max_request_results, 1_000);
        assert!(config.enable_streaming);
        assert_eq!(config.cleanup_interval_seconds, 30);
    }

    #[tokio::test]
    async fn test_streaming_collector_creation() {
        let config = MemoryConfig::streaming();
        let collector = StreamingStatsCollector::new(config);

        let memory_stats = collector.get_memory_stats().await;
        assert_eq!(memory_stats.current_requests_in_memory, 0);
        assert_eq!(memory_stats.total_requests_processed, 0);
    }

    #[tokio::test]
    async fn test_request_recording() {
        let config = MemoryConfig::streaming();
        let collector = StreamingStatsCollector::new(config);

        let result = RequestResult {
            timestamp: Utc::now(),
            duration_ms: 100,
            status_code: Some(200),
            error: None,
            user_agent: Some("test".to_string()),
            bytes_received: 1024,
        };

        collector.record_request(result).await;

        let memory_stats = collector.get_memory_stats().await;
        assert_eq!(memory_stats.current_requests_in_memory, 1);
        assert_eq!(memory_stats.total_requests_processed, 1);

        let aggregated_stats = collector.get_aggregated_stats().await;
        assert_eq!(aggregated_stats.total_requests, 1);
        assert_eq!(aggregated_stats.successful_requests, 1);
        assert_eq!(aggregated_stats.total_bytes_received, 1024);
    }

    #[tokio::test]
    async fn test_size_limits() {
        let mut config = MemoryConfig::streaming();
        config.max_request_results = 2; // Very small limit for testing

        let collector = StreamingStatsCollector::new(config);

        // Add 3 results
        for i in 0..3 {
            let result = RequestResult {
                timestamp: Utc::now(),
                duration_ms: 100 + i,
                status_code: Some(200),
                error: None,
                user_agent: Some("test".to_string()),
                bytes_received: 1024,
            };
            collector.record_request(result).await;
        }

        let memory_stats = collector.get_memory_stats().await;
        assert_eq!(memory_stats.current_requests_in_memory, 2); // Should be limited to 2

        let aggregated_stats = collector.get_aggregated_stats().await;
        assert_eq!(aggregated_stats.total_requests, 3); // Aggregated stats should have all 3
    }
}
