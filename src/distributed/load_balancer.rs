use crate::metrics::distributed_stats::WorkerMetrics;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Load distribution strategies for distributing work across workers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoadDistributionStrategy {
    /// Distribute requests evenly across all workers
    RoundRobin,
    /// Weight distribution based on worker capabilities (CPU, memory, max RPS)
    WeightedByCapacity,
    /// Dynamic distribution based on current worker load and performance
    LoadBased,
    /// Distribute based on geographic/network proximity (future enhancement)
    Geographic,
    /// Custom distribution with manual weights per worker
    Custom(HashMap<String, f64>),
}

impl Default for LoadDistributionStrategy {
    fn default() -> Self {
        Self::LoadBased
    }
}

/// Configuration for load distribution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadDistributionConfig {
    pub strategy: LoadDistributionStrategy,
    /// Minimum load threshold before rebalancing (0.0-1.0)
    pub rebalance_threshold: f64,
    /// How often to recalculate load distribution (seconds)
    pub recalculation_interval_secs: u64,
    /// Safety margin to prevent worker overload (0.0-1.0)
    pub safety_margin: f64,
    /// Enable automatic rebalancing when workers join/leave
    pub auto_rebalance: bool,
}

impl Default for LoadDistributionConfig {
    fn default() -> Self {
        Self {
            strategy: LoadDistributionStrategy::default(),
            rebalance_threshold: 0.2, // Rebalance if 20% imbalance
            recalculation_interval_secs: 30,
            safety_margin: 0.1, // Leave 10% capacity buffer
            auto_rebalance: true,
        }
    }
}

/// Recommended distribution of load across workers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadDistribution {
    /// Total load to distribute
    pub total_concurrent_requests: usize,
    pub total_rps: Option<u64>,
    /// Per-worker allocation
    pub worker_allocations: HashMap<String, WorkerAllocation>,
    /// Strategy used for this distribution
    pub strategy: LoadDistributionStrategy,
    /// Timestamp when distribution was calculated
    pub calculated_at: chrono::DateTime<chrono::Utc>,
    /// Confidence score of the distribution (0.0-1.0)
    pub confidence_score: f64,
}

/// Allocation of work for a specific worker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAllocation {
    pub worker_id: String,
    /// Recommended concurrent requests for this worker
    pub concurrent_requests: usize,
    /// Recommended RPS limit for this worker
    pub rps_limit: Option<u64>,
    /// Weight assigned to this worker (0.0-1.0)
    pub weight: f64,
    /// Utilization percentage expected with this allocation
    pub expected_utilization: f64,
    /// Justification for this allocation
    pub allocation_reason: String,
}

/// Load distribution engine that calculates optimal load distribution
pub struct LoadDistributionEngine {
    config: LoadDistributionConfig,
}

impl LoadDistributionEngine {
    pub fn new(config: LoadDistributionConfig) -> Self {
        Self { config }
    }

    /// Calculate optimal load distribution across available workers
    pub fn calculate_distribution(
        &self,
        workers: &[WorkerMetrics],
        total_concurrent: usize,
        total_rps: Option<u64>,
    ) -> LoadDistribution {
        let active_workers: Vec<_> = workers
            .iter()
            .filter(|w| matches!(w.status.as_str(), "Running" | "Idle" | "Connected"))
            .collect();

        if active_workers.is_empty() {
            return LoadDistribution {
                total_concurrent_requests: total_concurrent,
                total_rps,
                worker_allocations: HashMap::new(),
                strategy: self.config.strategy.clone(),
                calculated_at: chrono::Utc::now(),
                confidence_score: 0.0,
            };
        }

        let worker_allocations = match &self.config.strategy {
            LoadDistributionStrategy::RoundRobin => {
                self.calculate_round_robin(&active_workers, total_concurrent, total_rps)
            }
            LoadDistributionStrategy::WeightedByCapacity => {
                self.calculate_weighted_by_capacity(&active_workers, total_concurrent, total_rps)
            }
            LoadDistributionStrategy::LoadBased => {
                self.calculate_load_based(&active_workers, total_concurrent, total_rps)
            }
            LoadDistributionStrategy::Geographic => {
                // Future implementation - for now, fallback to load-based
                self.calculate_load_based(&active_workers, total_concurrent, total_rps)
            }
            LoadDistributionStrategy::Custom(weights) => {
                self.calculate_custom(&active_workers, weights, total_concurrent, total_rps)
            }
        };

        let confidence_score =
            self.calculate_confidence_score(&active_workers, &worker_allocations);

        LoadDistribution {
            total_concurrent_requests: total_concurrent,
            total_rps,
            worker_allocations,
            strategy: self.config.strategy.clone(),
            calculated_at: chrono::Utc::now(),
            confidence_score,
        }
    }

    /// Round-robin distribution: equal load for all workers
    fn calculate_round_robin(
        &self,
        workers: &[&WorkerMetrics],
        total_concurrent: usize,
        total_rps: Option<u64>,
    ) -> HashMap<String, WorkerAllocation> {
        let worker_count = workers.len();
        let concurrent_per_worker = total_concurrent / worker_count;
        let remainder = total_concurrent % worker_count;
        let rps_per_worker = total_rps.map(|rps| rps / worker_count as u64);

        let mut allocations = HashMap::new();

        for (i, worker) in workers.iter().enumerate() {
            let extra_concurrent = if i < remainder { 1 } else { 0 };
            let concurrent = concurrent_per_worker + extra_concurrent;

            allocations.insert(
                worker.worker_id.clone(),
                WorkerAllocation {
                    worker_id: worker.worker_id.clone(),
                    concurrent_requests: concurrent,
                    rps_limit: rps_per_worker,
                    weight: 1.0 / worker_count as f64,
                    expected_utilization: 0.5, // Assume 50% utilization for round-robin
                    allocation_reason: "Round-robin distribution".to_string(),
                },
            );
        }

        allocations
    }

    /// Weighted distribution based on worker hardware capabilities
    fn calculate_weighted_by_capacity(
        &self,
        workers: &[&WorkerMetrics],
        total_concurrent: usize,
        total_rps: Option<u64>,
    ) -> HashMap<String, WorkerAllocation> {
        // Calculate capacity scores for each worker
        let capacity_scores: Vec<f64> = workers
            .iter()
            .map(|w| self.calculate_capacity_score(w))
            .collect();

        let total_capacity: f64 = capacity_scores.iter().sum();
        let mut allocations = HashMap::new();

        for (i, worker) in workers.iter().enumerate() {
            let weight = capacity_scores[i] / total_capacity;
            let concurrent = ((total_concurrent as f64 * weight).round() as usize).max(1);
            let rps_limit = total_rps.map(|rps| ((rps as f64 * weight).round() as u64).max(1));

            let max_capacity = worker.metrics.active_connections.max(1) as f64
                + (worker.load.memory_usage_mb / 100) as f64; // Simplified capacity calculation
            let expected_utilization = (concurrent as f64 / max_capacity).min(1.0);

            allocations.insert(
                worker.worker_id.clone(),
                WorkerAllocation {
                    worker_id: worker.worker_id.clone(),
                    concurrent_requests: concurrent,
                    rps_limit,
                    weight,
                    expected_utilization,
                    allocation_reason: format!(
                        "Capacity-weighted (score: {:.2})",
                        capacity_scores[i]
                    ),
                },
            );
        }

        allocations
    }

    /// Dynamic distribution based on current worker load and performance
    fn calculate_load_based(
        &self,
        workers: &[&WorkerMetrics],
        total_concurrent: usize,
        total_rps: Option<u64>,
    ) -> HashMap<String, WorkerAllocation> {
        // Calculate load scores (lower is better)
        let load_scores: Vec<f64> = workers
            .iter()
            .map(|w| self.calculate_load_score(w))
            .collect();

        // Invert load scores to get capacity scores (higher is better)
        let max_load = load_scores.iter().fold(0.0_f64, |a, &b| a.max(b));
        let capacity_scores: Vec<f64> = load_scores
            .iter()
            .map(|&score| (max_load + 0.1) - score) // Add small constant to avoid division by zero
            .collect();

        let total_capacity: f64 = capacity_scores.iter().sum();
        let mut allocations = HashMap::new();

        for (i, worker) in workers.iter().enumerate() {
            let weight = capacity_scores[i] / total_capacity;
            let concurrent = ((total_concurrent as f64 * weight).round() as usize).max(1);
            let rps_limit = total_rps.map(|rps| ((rps as f64 * weight).round() as u64).max(1));

            let expected_utilization = load_scores[i] + (weight * 0.5); // Estimate new utilization

            // Apply safety margin only if utilization is expected to be high
            let final_concurrent = if expected_utilization > 0.7 {
                ((concurrent as f64 * (1.0 - self.config.safety_margin)).round() as usize).max(1)
            } else {
                concurrent
            };

            allocations.insert(
                worker.worker_id.clone(),
                WorkerAllocation {
                    worker_id: worker.worker_id.clone(),
                    concurrent_requests: final_concurrent,
                    rps_limit,
                    weight,
                    expected_utilization: expected_utilization.min(1.0),
                    allocation_reason: format!("Load-based (current load: {:.2})", load_scores[i]),
                },
            );
        }

        allocations
    }

    /// Custom distribution with user-defined weights
    fn calculate_custom(
        &self,
        workers: &[&WorkerMetrics],
        weights: &HashMap<String, f64>,
        total_concurrent: usize,
        total_rps: Option<u64>,
    ) -> HashMap<String, WorkerAllocation> {
        let mut allocations = HashMap::new();
        let total_weight: f64 = weights.values().sum();

        if total_weight == 0.0 {
            // Fallback to round-robin if no valid weights
            return self.calculate_round_robin(workers, total_concurrent, total_rps);
        }

        for worker in workers {
            let weight = weights.get(&worker.worker_id).copied().unwrap_or(0.0);
            let normalized_weight = weight / total_weight;
            let concurrent =
                ((total_concurrent as f64 * normalized_weight).round() as usize).max(1);
            let rps_limit =
                total_rps.map(|rps| ((rps as f64 * normalized_weight).round() as u64).max(1));

            allocations.insert(
                worker.worker_id.clone(),
                WorkerAllocation {
                    worker_id: worker.worker_id.clone(),
                    concurrent_requests: concurrent,
                    rps_limit,
                    weight: normalized_weight,
                    expected_utilization: 0.6, // Assume moderate utilization for custom weights
                    allocation_reason: format!("Custom weight: {:.2}", weight),
                },
            );
        }

        allocations
    }

    /// Calculate capacity score based on worker hardware and configuration
    fn calculate_capacity_score(&self, worker: &WorkerMetrics) -> f64 {
        // This is a simplified capacity calculation
        // In practice, you might want more sophisticated algorithms

        let cpu_score = 4.0; // Assume 4 cores if not available
        let memory_score = (worker.load.memory_usage_mb as f64 / 1024.0).max(1.0); // GB
        let connection_capacity = 1000.0; // Assume 1000 max connections if not specified

        // Combine scores with weights
        cpu_score * 0.4 + memory_score * 0.3 + connection_capacity * 0.3
    }

    /// Calculate current load score (0.0 = no load, 1.0 = fully loaded)
    fn calculate_load_score(&self, worker: &WorkerMetrics) -> f64 {
        let cpu_load = worker.load.cpu_usage_percent / 100.0;
        let memory_load = worker.load.memory_usage_mb as f64 / 8192.0; // Assume 8GB total
        let connection_load = worker.load.active_connections as f64 / 1000.0; // Assume 1000 max
        let rps_load = if worker.metrics.current_rps > 0.0 {
            worker.metrics.current_rps / 100.0 // Assume 100 RPS baseline
        } else {
            0.0
        };

        // Weighted average of load factors
        (cpu_load * 0.3 + memory_load * 0.2 + connection_load * 0.3 + rps_load * 0.2).min(1.0)
    }

    /// Calculate confidence score for the distribution
    fn calculate_confidence_score(
        &self,
        workers: &[&WorkerMetrics],
        allocations: &HashMap<String, WorkerAllocation>,
    ) -> f64 {
        if workers.is_empty() || allocations.is_empty() {
            return 0.0;
        }

        // Factors that affect confidence:
        // 1. Number of workers (more workers = more confidence)
        // 2. Load balance (more balanced = higher confidence)
        // 3. Worker stability (longer running = higher confidence)

        let worker_count_factor = (workers.len() as f64).min(10.0) / 10.0;

        // Calculate load balance factor
        let weights: Vec<f64> = allocations.values().map(|a| a.weight).collect();
        let avg_weight = weights.iter().sum::<f64>() / weights.len() as f64;
        let weight_variance = weights
            .iter()
            .map(|w| (w - avg_weight).powi(2))
            .sum::<f64>()
            / weights.len() as f64;
        let balance_factor = 1.0 - weight_variance.min(1.0);

        // Simple stability factor based on worker count
        let stability_factor = if workers.len() >= 3 { 0.9 } else { 0.7 };

        (worker_count_factor * 0.4 + balance_factor * 0.4 + stability_factor * 0.2).min(1.0)
    }

    /// Check if load rebalancing is needed
    pub fn should_rebalance(
        &self,
        current_distribution: &LoadDistribution,
        current_workers: &[WorkerMetrics],
    ) -> bool {
        if !self.config.auto_rebalance {
            return false;
        }

        // Rebalance if worker count changed
        let active_workers = current_workers
            .iter()
            .filter(|w| matches!(w.status.as_str(), "Running" | "Idle" | "Connected"))
            .count();

        if active_workers != current_distribution.worker_allocations.len() {
            return true;
        }

        // Rebalance if load imbalance exceeds threshold
        if current_distribution.confidence_score < (1.0 - self.config.rebalance_threshold) {
            return true;
        }

        // Check if any worker is overloaded
        for worker in current_workers {
            if let Some(allocation) = current_distribution
                .worker_allocations
                .get(&worker.worker_id)
            {
                let current_load = self.calculate_load_score(worker);
                if current_load > 0.8 && allocation.expected_utilization < 0.6 {
                    return true; // Worker is more loaded than expected
                }
            }
        }

        false
    }

    /// Get configuration
    pub fn get_config(&self) -> &LoadDistributionConfig {
        &self.config
    }

    /// Update configuration
    pub fn update_config(&mut self, config: LoadDistributionConfig) {
        self.config = config;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integrations::websocket::WorkerLoad;
    use crate::metrics::{LatencyHistogram, LiveMetrics};
    use std::collections::HashMap;

    fn create_test_worker(
        id: &str,
        cpu_usage: f64,
        memory_mb: u64,
        active_connections: usize,
    ) -> WorkerMetrics {
        WorkerMetrics {
            worker_id: id.to_string(),
            status: "Running".to_string(),
            metrics: LiveMetrics {
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
                active_connections: active_connections as u64,
                queue_size: 0,
                bytes_received: 1024,
                status_codes: HashMap::new(),
                errors: HashMap::new(),
                latency_histogram: LatencyHistogram::new(),
                active_alerts: Vec::new(),
            },
            load: WorkerLoad {
                current_rps: 10.0,
                active_connections,
                memory_usage_mb: memory_mb,
                cpu_usage_percent: cpu_usage,
                total_requests_sent: 100,
                errors_count: 5,
            },
            last_update: chrono::Utc::now(),
            connection_time: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_round_robin_distribution() {
        let config = LoadDistributionConfig {
            strategy: LoadDistributionStrategy::RoundRobin,
            ..Default::default()
        };
        let engine = LoadDistributionEngine::new(config);

        let workers = vec![
            create_test_worker("worker1", 10.0, 1024, 10),
            create_test_worker("worker2", 20.0, 2048, 20),
            create_test_worker("worker3", 15.0, 1536, 15),
        ];

        let distribution = engine.calculate_distribution(&workers, 30, Some(60));

        assert_eq!(distribution.worker_allocations.len(), 3);

        // Each worker should get 10 concurrent requests in round-robin
        for allocation in distribution.worker_allocations.values() {
            assert_eq!(allocation.concurrent_requests, 10);
            assert_eq!(allocation.rps_limit, Some(20));
            assert!((allocation.weight - 0.333).abs() < 0.01); // Approximately 1/3
        }
    }

    #[test]
    fn test_load_based_distribution() {
        let mut config = LoadDistributionConfig::default();
        config.strategy = LoadDistributionStrategy::LoadBased;
        let engine = LoadDistributionEngine::new(config);

        let workers = vec![
            create_test_worker("worker1", 10.0, 1024, 10), // Low load
            create_test_worker("worker2", 80.0, 7000, 800), // High load
            create_test_worker("worker3", 30.0, 3000, 100), // Medium load
        ];

        let distribution = engine.calculate_distribution(&workers, 30, Some(60));

        assert_eq!(distribution.worker_allocations.len(), 3);

        // Worker1 (low load) should get more work than worker2 (high load)
        let worker1_concurrent = distribution
            .worker_allocations
            .get("worker1")
            .unwrap()
            .concurrent_requests;
        let worker2_concurrent = distribution
            .worker_allocations
            .get("worker2")
            .unwrap()
            .concurrent_requests;

        assert!(worker1_concurrent > worker2_concurrent);
    }

    #[test]
    fn test_custom_distribution() {
        let mut custom_weights = HashMap::new();
        custom_weights.insert("worker1".to_string(), 0.5);
        custom_weights.insert("worker2".to_string(), 0.3);
        custom_weights.insert("worker3".to_string(), 0.2);

        let config = LoadDistributionConfig {
            strategy: LoadDistributionStrategy::Custom(custom_weights),
            ..Default::default()
        };
        let engine = LoadDistributionEngine::new(config);

        let workers = vec![
            create_test_worker("worker1", 10.0, 1024, 10),
            create_test_worker("worker2", 20.0, 2048, 20),
            create_test_worker("worker3", 15.0, 1536, 15),
        ];

        let distribution = engine.calculate_distribution(&workers, 100, Some(100));

        // Worker1 should get 50% of load
        let worker1_allocation = distribution.worker_allocations.get("worker1").unwrap();
        assert_eq!(worker1_allocation.concurrent_requests, 50);
        assert_eq!(worker1_allocation.rps_limit, Some(50));

        // Worker2 should get 30% of load
        let worker2_allocation = distribution.worker_allocations.get("worker2").unwrap();
        assert_eq!(worker2_allocation.concurrent_requests, 30);
        assert_eq!(worker2_allocation.rps_limit, Some(30));

        // Worker3 should get 20% of load
        let worker3_allocation = distribution.worker_allocations.get("worker3").unwrap();
        assert_eq!(worker3_allocation.concurrent_requests, 20);
        assert_eq!(worker3_allocation.rps_limit, Some(20));
    }

    #[test]
    fn test_should_rebalance() {
        let config = LoadDistributionConfig {
            rebalance_threshold: 0.5, // More permissive threshold
            ..Default::default()
        };
        let engine = LoadDistributionEngine::new(config);

        let workers = vec![
            create_test_worker("worker1", 10.0, 1024, 10),
            create_test_worker("worker2", 20.0, 2048, 20),
        ];

        let distribution = engine.calculate_distribution(&workers, 20, Some(40));

        // Should not rebalance with same workers
        assert!(!engine.should_rebalance(&distribution, &workers));

        // Should rebalance when worker count changes
        let new_workers = vec![
            create_test_worker("worker1", 10.0, 1024, 10),
            create_test_worker("worker2", 20.0, 2048, 20),
            create_test_worker("worker3", 15.0, 1536, 15),
        ];
        assert!(engine.should_rebalance(&distribution, &new_workers));
    }

    #[test]
    fn test_confidence_score() {
        let config = LoadDistributionConfig::default();
        let engine = LoadDistributionEngine::new(config);

        // Test with no workers
        let distribution = engine.calculate_distribution(&[], 10, Some(20));
        assert_eq!(distribution.confidence_score, 0.0);

        // Test with multiple workers
        let workers = vec![
            create_test_worker("worker1", 10.0, 1024, 10),
            create_test_worker("worker2", 20.0, 2048, 20),
            create_test_worker("worker3", 15.0, 1536, 15),
        ];

        let distribution = engine.calculate_distribution(&workers, 30, Some(60));
        assert!(distribution.confidence_score > 0.0);
        assert!(distribution.confidence_score <= 1.0);
    }
}
