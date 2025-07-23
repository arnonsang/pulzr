use super::load_balancer::{LoadDistribution, LoadDistributionConfig, LoadDistributionEngine};
use crate::integrations::websocket::{
    CoordinationSettings, CoordinatorTestStatus, DistributedTestConfig, SyncState, TestCommandType,
    WebSocketMessage, WorkerInfo, WorkerLoad, WorkerStatus,
};
use crate::metrics::{DistributedStatsCollector, StatsCollector};
use crate::utils::port_utils::find_available_port;
use anyhow::Result;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio::time::{interval, sleep, Duration, Instant};
use tokio_tungstenite::accept_async;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct ConnectedWorker {
    pub worker_id: String,
    pub worker_info: WorkerInfo,
    pub status: WorkerStatus,
    pub last_heartbeat: Instant,
    pub current_load: WorkerLoad,
    pub connection_time: Instant,
}

#[derive(Debug, Clone)]
pub struct SyncTracker {
    pub test_id: String,
    pub sync_state: SyncState,
    pub target_workers: Vec<String>,
    pub ready_workers: HashMap<String, Instant>,
    pub coordination_settings: CoordinationSettings,
    pub sync_start_time: Instant,
    pub sync_timeout: Duration,
}

struct WorkerConnectionContext {
    workers: Arc<RwLock<HashMap<String, ConnectedWorker>>>,
    message_sender: broadcast::Sender<WebSocketMessage>,
    coordinator_id: String,
    max_workers: usize,
    distributed_stats: Arc<DistributedStatsCollector>,
    sync_tracker: Arc<RwLock<Option<SyncTracker>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    pub coordinator_id: String,
    pub port: u16,
    pub max_workers: usize,
    pub heartbeat_timeout_secs: u64,
    pub heartbeat_check_interval_secs: u64,
    pub auto_balance_load: bool,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            coordinator_id: format!("coordinator-{}", Uuid::new_v4()),
            port: 9630,
            max_workers: 100,
            heartbeat_timeout_secs: 30,
            heartbeat_check_interval_secs: 10,
            auto_balance_load: true,
        }
    }
}

pub struct DistributedCoordinator {
    config: CoordinatorConfig,
    actual_port: Option<u16>,
    workers: Arc<RwLock<HashMap<String, ConnectedWorker>>>,
    test_status: Arc<RwLock<CoordinatorTestStatus>>,
    current_test_id: Arc<RwLock<Option<String>>>,
    sync_tracker: Arc<RwLock<Option<SyncTracker>>>,
    distributed_stats: Arc<DistributedStatsCollector>,
    load_balancer: Arc<RwLock<LoadDistributionEngine>>,
    current_distribution: Arc<RwLock<Option<LoadDistribution>>>,
    message_sender: broadcast::Sender<WebSocketMessage>,
}

impl DistributedCoordinator {
    pub fn new(config: CoordinatorConfig, _stats_collector: Arc<StatsCollector>) -> Self {
        let (message_sender, _) = broadcast::channel(1000);
        let distributed_stats = Arc::new(
            DistributedStatsCollector::new().with_websocket_sender(message_sender.clone()),
        );

        // Initialize load balancer with default configuration
        let load_balancer_config = LoadDistributionConfig::default();
        let load_balancer = Arc::new(RwLock::new(LoadDistributionEngine::new(
            load_balancer_config,
        )));

        Self {
            config,
            actual_port: None,
            workers: Arc::new(RwLock::new(HashMap::new())),
            test_status: Arc::new(RwLock::new(CoordinatorTestStatus::Idle)),
            current_test_id: Arc::new(RwLock::new(None)),
            sync_tracker: Arc::new(RwLock::new(None)),
            distributed_stats,
            load_balancer,
            current_distribution: Arc::new(RwLock::new(None)),
            message_sender,
        }
    }

    pub async fn start(&mut self) -> Result<u16> {
        let actual_port = find_available_port(self.config.port, 50).ok_or_else(|| {
            anyhow::anyhow!(
                "Could not find available port starting from {}",
                self.config.port
            )
        })?;

        self.actual_port = Some(actual_port);

        let addr = SocketAddr::from(([0, 0, 0, 0], actual_port)); // Listen on all interfaces
        let listener = TcpListener::bind(&addr).await?;

        println!(
            "Distributed Load Testing Coordinator listening on: ws://{}",
            addr
        );
        println!("Coordinator ID: {}", self.config.coordinator_id);

        // Start heartbeat monitoring task
        self.start_heartbeat_monitor().await;

        // Start failure recovery and load rebalancing task
        self.start_failure_recovery().await;

        // Start status broadcasting task
        self.start_status_broadcaster().await;

        // Accept worker connections
        let workers = Arc::clone(&self.workers);
        let message_sender = self.message_sender.clone();
        let coordinator_id = self.config.coordinator_id.clone();
        let max_workers = self.config.max_workers;
        let distributed_stats = Arc::clone(&self.distributed_stats);
        let sync_tracker = Arc::clone(&self.sync_tracker);

        tokio::spawn(async move {
            while let Ok((stream, addr)) = listener.accept().await {
                println!("New connection from: {}", addr);

                let connection_context = WorkerConnectionContext {
                    workers: Arc::clone(&workers),
                    message_sender: message_sender.clone(),
                    coordinator_id: coordinator_id.clone(),
                    max_workers,
                    distributed_stats: Arc::clone(&distributed_stats),
                    sync_tracker: Arc::clone(&sync_tracker),
                };

                tokio::spawn(handle_worker_connection(stream, addr, connection_context));
            }
        });

        Ok(actual_port)
    }

    async fn start_heartbeat_monitor(&self) {
        let workers = Arc::clone(&self.workers);
        let timeout_duration = Duration::from_secs(self.config.heartbeat_timeout_secs);
        let heartbeat_check_interval = self.config.heartbeat_check_interval_secs;
        let message_sender = self.message_sender.clone();
        let distributed_stats = Arc::clone(&self.distributed_stats);

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(heartbeat_check_interval)); // Use configurable interval

            loop {
                interval.tick().await;
                let now = Instant::now();
                let mut workers_guard = workers.write().await;
                let mut disconnected_workers = Vec::new();

                for (worker_id, worker) in workers_guard.iter() {
                    if now.duration_since(worker.last_heartbeat) > timeout_duration {
                        disconnected_workers.push(worker_id.clone());
                    }
                }

                for worker_id in disconnected_workers {
                    if let Some(worker) = workers_guard.remove(&worker_id) {
                        println!(
                            "⚠️  Worker {} failed: Heartbeat timeout after {}s (was {:?} for {:?})",
                            worker_id,
                            timeout_duration.as_secs(),
                            worker.status,
                            now.duration_since(worker.connection_time)
                        );

                        // Remove from distributed stats
                        distributed_stats.remove_worker(&worker_id).await;

                        // Create detailed failure event
                        let failure_msg = WebSocketMessage::WorkerFailure {
                            timestamp: Utc::now(),
                            worker_id: worker_id.clone(),
                            reason: "Heartbeat timeout".to_string(),
                            last_seen: worker.last_heartbeat.elapsed().as_secs(),
                            worker_info: worker.worker_info.clone(),
                        };
                        let _ = message_sender.send(failure_msg);

                        // Also send disconnect message for compatibility
                        let disconnect_msg = WebSocketMessage::WorkerDisconnect {
                            timestamp: Utc::now(),
                            worker_id: worker_id.clone(),
                            reason: "Heartbeat timeout".to_string(),
                        };
                        let _ = message_sender.send(disconnect_msg);
                    }
                }
            }
        });
    }

    async fn start_failure_recovery(&self) {
        let workers = Arc::clone(&self.workers);
        let test_status = Arc::clone(&self.test_status);
        let current_distribution = Arc::clone(&self.current_distribution);
        let load_balancer = Arc::clone(&self.load_balancer);
        let message_sender = self.message_sender.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(15)); // Check every 15 seconds

            loop {
                interval.tick().await;

                let status = {
                    let status_guard = test_status.read().await;
                    status_guard.clone()
                };

                // Only perform recovery during active testing
                if matches!(status, CoordinatorTestStatus::Running) {
                    let workers_guard = workers.read().await;
                    let active_workers: Vec<String> = workers_guard.keys().cloned().collect();
                    let worker_count = active_workers.len();
                    drop(workers_guard);

                    // Check if we need to rebalance load due to worker changes
                    let should_rebalance = {
                        let distribution_guard = current_distribution.read().await;
                        if let Some(ref distribution) = *distribution_guard {
                            // Check if current worker set differs from distribution
                            let distribution_workers: std::collections::HashSet<String> =
                                distribution.worker_allocations.keys().cloned().collect();
                            let current_workers: std::collections::HashSet<String> =
                                active_workers.iter().cloned().collect();

                            distribution_workers != current_workers
                        } else {
                            false
                        }
                    };

                    if should_rebalance && worker_count > 0 {
                        println!(
                            "🔄 Rebalancing load after worker changes ({} workers active)",
                            worker_count
                        );

                        // Get load balancer configuration
                        let (_strategy, total_target_load) = {
                            let balancer_guard = load_balancer.read().await;
                            (balancer_guard.get_config().strategy.clone(), 1000)
                            // Example target load
                        };

                        // Create new distribution for remaining workers - using simplified approach for recovery
                        let balancer_guard = load_balancer.read().await;
                        let worker_metrics: Vec<_> = active_workers
                            .iter()
                            .map(|worker_id| {
                                use crate::integrations::websocket::WorkerLoad;
                                use crate::metrics::stats::LiveMetrics;

                                crate::metrics::distributed_stats::WorkerMetrics {
                                    worker_id: worker_id.clone(),
                                    status: "Idle".to_string(),
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
                                        status_codes: std::collections::HashMap::new(),
                                        errors: std::collections::HashMap::new(),
                                        latency_histogram:
                                            crate::metrics::stats::LatencyHistogram::new(),
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
                                    last_update: chrono::Utc::now(),
                                    connection_time: chrono::Utc::now(),
                                }
                            })
                            .collect();

                        let new_distribution = balancer_guard.calculate_distribution(
                            &worker_metrics,
                            total_target_load,
                            None,
                        );
                        let mut distribution_guard = current_distribution.write().await;
                        *distribution_guard = Some(new_distribution.clone());
                        drop(distribution_guard);
                        drop(balancer_guard);

                        // Broadcast rebalancing event
                        let rebalance_msg = WebSocketMessage::LoadRebalanced {
                            timestamp: chrono::Utc::now(),
                            active_workers: active_workers.clone(),
                            new_distribution,
                            reason: "Worker failure recovery".to_string(),
                        };
                        let _ = message_sender.send(rebalance_msg);

                        println!("✅ Load rebalanced across {} workers", worker_count);
                    }

                    // Check for worker performance issues and potential preemptive failures
                    let workers_guard = workers.read().await;
                    for (worker_id, worker) in workers_guard.iter() {
                        let last_heartbeat_age = worker.last_heartbeat.elapsed();

                        // Warn about workers that are approaching timeout
                        if last_heartbeat_age > Duration::from_secs(20)
                            && last_heartbeat_age < Duration::from_secs(25)
                        {
                            println!(
                                "⚠️  Worker {} heartbeat delayed: {}s (timeout at 30s)",
                                worker_id,
                                last_heartbeat_age.as_secs()
                            );

                            let warning_msg = WebSocketMessage::WorkerWarning {
                                timestamp: chrono::Utc::now(),
                                worker_id: worker_id.clone(),
                                warning_type: "heartbeat_delay".to_string(),
                                message: format!(
                                    "Heartbeat delayed {}s",
                                    last_heartbeat_age.as_secs()
                                ),
                            };
                            let _ = message_sender.send(warning_msg);
                        }
                    }
                }

                // Small delay to prevent tight loop
                sleep(Duration::from_millis(100)).await;
            }
        });
    }

    async fn start_status_broadcaster(&self) {
        let workers = Arc::clone(&self.workers);
        let test_status = Arc::clone(&self.test_status);
        let message_sender = self.message_sender.clone();
        let coordinator_id = self.config.coordinator_id.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(5)); // Broadcast every 5 seconds

            loop {
                interval.tick().await;

                let workers_guard = workers.read().await;
                let connected_worker_ids: Vec<String> = workers_guard.keys().cloned().collect();
                let status = test_status.read().await.clone();
                drop(workers_guard);

                let status_msg = WebSocketMessage::CoordinatorStatus {
                    timestamp: Utc::now(),
                    coordinator_id: coordinator_id.clone(),
                    connected_workers: connected_worker_ids,
                    test_status: status,
                };

                let _ = message_sender.send(status_msg);
            }
        });
    }

    pub async fn start_distributed_test(&self, test_config: DistributedTestConfig) -> Result<()> {
        let mut status_guard = self.test_status.write().await;
        if !matches!(*status_guard, CoordinatorTestStatus::Idle) {
            return Err(anyhow::anyhow!("Another test is already running"));
        }

        *status_guard = CoordinatorTestStatus::Preparing;
        drop(status_guard);

        let mut test_id_guard = self.current_test_id.write().await;
        *test_id_guard = Some(test_config.test_id.clone());
        drop(test_id_guard);

        // Get available workers
        let workers_guard = self.workers.read().await;
        let available_workers: Vec<String> = workers_guard
            .iter()
            .filter(|(_, worker)| matches!(worker.status, WorkerStatus::Idle))
            .map(|(id, _)| id.clone())
            .collect();
        drop(workers_guard);

        if available_workers.is_empty() {
            let mut status_guard = self.test_status.write().await;
            *status_guard = CoordinatorTestStatus::Failed;
            return Err(anyhow::anyhow!("No available workers for testing"));
        }

        // Check if synchronized start is enabled
        if test_config.coordination_settings.synchronized_start {
            self.start_synchronized_test(test_config, available_workers)
                .await
        } else {
            self.start_unsynchronized_test(test_config, available_workers)
                .await
        }
    }

    async fn start_unsynchronized_test(
        &self,
        test_config: DistributedTestConfig,
        available_workers: Vec<String>,
    ) -> Result<()> {
        let command_id = Uuid::new_v4().to_string();
        let test_command = WebSocketMessage::TestCommand {
            timestamp: Utc::now(),
            command_id,
            command_type: TestCommandType::Start,
            test_config,
            target_workers: available_workers,
        };

        self.message_sender.send(test_command)?;

        let mut status_guard = self.test_status.write().await;
        *status_guard = CoordinatorTestStatus::Running;

        println!("Distributed test started successfully (unsynchronized)");
        Ok(())
    }

    async fn start_synchronized_test(
        &self,
        test_config: DistributedTestConfig,
        available_workers: Vec<String>,
    ) -> Result<()> {
        println!(
            "Starting synchronized test with {} workers",
            available_workers.len()
        );

        // Initialize sync tracker
        let sync_timeout = Duration::from_secs(test_config.coordination_settings.sync_timeout_secs);
        let mut sync_tracker_guard = self.sync_tracker.write().await;
        *sync_tracker_guard = Some(SyncTracker {
            test_id: test_config.test_id.clone(),
            sync_state: SyncState::Preparing,
            target_workers: available_workers.clone(),
            ready_workers: HashMap::new(),
            coordination_settings: test_config.coordination_settings.clone(),
            sync_start_time: Instant::now(),
            sync_timeout,
        });
        drop(sync_tracker_guard);

        // Send prepare message to all target workers
        let prepare_message = WebSocketMessage::SyncPrepare {
            timestamp: Utc::now(),
            test_id: test_config.test_id.clone(),
            coordinator_id: self.config.coordinator_id.clone(),
            target_workers: available_workers.clone(),
            sync_timeout_secs: test_config.coordination_settings.sync_timeout_secs,
        };

        self.message_sender.send(prepare_message)?;

        // Send test command to workers (they will wait for sync start signal)
        let command_id = Uuid::new_v4().to_string();
        let test_command = WebSocketMessage::TestCommand {
            timestamp: Utc::now(),
            command_id,
            command_type: TestCommandType::Start,
            test_config,
            target_workers: available_workers,
        };

        self.message_sender.send(test_command)?;

        // Start sync monitoring task
        self.start_sync_monitor().await;

        println!("Synchronized test preparation initiated");
        Ok(())
    }

    async fn start_sync_monitor(&self) {
        let sync_tracker = Arc::clone(&self.sync_tracker);
        let message_sender = self.message_sender.clone();
        let test_status = Arc::clone(&self.test_status);
        let coordinator_id = self.config.coordinator_id.clone();

        tokio::spawn(async move {
            loop {
                sleep(Duration::from_millis(500)).await;

                let sync_guard = sync_tracker.read().await;
                if let Some(ref tracker) = *sync_guard {
                    let elapsed = tracker.sync_start_time.elapsed();

                    // Check for timeout
                    if elapsed > tracker.sync_timeout {
                        let failed_workers: Vec<String> = tracker
                            .target_workers
                            .iter()
                            .filter(|w| !tracker.ready_workers.contains_key(*w))
                            .cloned()
                            .collect();

                        let timeout_message = WebSocketMessage::SyncTimeout {
                            timestamp: Utc::now(),
                            test_id: tracker.test_id.clone(),
                            coordinator_id: coordinator_id.clone(),
                            timeout_reason: format!("Sync timeout after {}s", elapsed.as_secs()),
                            failed_workers,
                        };

                        let _ = message_sender.send(timeout_message);

                        // Update test status to failed
                        let mut status_guard = test_status.write().await;
                        *status_guard = CoordinatorTestStatus::Failed;

                        println!("❌ Synchronization timeout after {}s", elapsed.as_secs());
                        break;
                    }

                    // Check if all workers are ready
                    if tracker.ready_workers.len() == tracker.target_workers.len()
                        && matches!(tracker.sync_state, SyncState::Preparing)
                    {
                        let start_timestamp = Utc::now();
                        let start_message = WebSocketMessage::SyncStart {
                            timestamp: start_timestamp,
                            test_id: tracker.test_id.clone(),
                            coordinator_id: coordinator_id.clone(),
                            target_workers: tracker.target_workers.clone(),
                            start_timestamp,
                        };

                        if message_sender.send(start_message).is_ok() {
                            // Update test status to running
                            let mut status_guard = test_status.write().await;
                            *status_guard = CoordinatorTestStatus::Running;

                            println!("✅ All workers ready - synchronous test started!");
                            break;
                        }
                    }
                } else {
                    // No sync tracker, exit
                    break;
                }
            }
        });
    }

    pub async fn stop_distributed_test(&self) -> Result<()> {
        let mut status_guard = self.test_status.write().await;
        if !matches!(*status_guard, CoordinatorTestStatus::Running) {
            return Err(anyhow::anyhow!("No test is currently running"));
        }

        *status_guard = CoordinatorTestStatus::Stopping;
        drop(status_guard);

        // Check if we should stop synchronously
        let sync_guard = self.sync_tracker.read().await;
        let should_sync_stop = if let Some(ref tracker) = *sync_guard {
            tracker.coordination_settings.synchronized_stop
        } else {
            false
        };
        drop(sync_guard);

        if should_sync_stop {
            self.stop_synchronized_test().await
        } else {
            self.stop_unsynchronized_test().await
        }
    }

    async fn stop_unsynchronized_test(&self) -> Result<()> {
        let command_id = Uuid::new_v4().to_string();
        let stop_command = WebSocketMessage::TestCommand {
            timestamp: Utc::now(),
            command_id,
            command_type: TestCommandType::Stop,
            test_config: DistributedTestConfig {
                test_id: "stop".to_string(),
                base_config: crate::integrations::websocket::TestConfig {
                    url: "".to_string(),
                    concurrent_requests: 0,
                    rps: None,
                    duration_secs: 0,
                    method: "GET".to_string(),
                    user_agent_mode: "default".to_string(),
                },
                worker_assignments: Vec::new(),
                coordination_settings: CoordinationSettings {
                    synchronized_start: false,
                    synchronized_stop: false,
                    sync_timeout_secs: 30,
                    max_sync_wait_secs: 60,
                    heartbeat_interval_secs: 10,
                    metrics_reporting_interval_secs: 5,
                    timeout_secs: 300,
                },
            },
            target_workers: Vec::new(), // All workers
        };

        self.message_sender.send(stop_command)?;

        let mut status_guard = self.test_status.write().await;
        *status_guard = CoordinatorTestStatus::Completed;

        println!("Stop command sent to all workers (unsynchronized)");
        Ok(())
    }

    async fn stop_synchronized_test(&self) -> Result<()> {
        let sync_guard = self.sync_tracker.read().await;
        let (test_id, target_workers) = if let Some(ref tracker) = *sync_guard {
            (tracker.test_id.clone(), tracker.target_workers.clone())
        } else {
            return Err(anyhow::anyhow!("No active synchronized test to stop"));
        };
        drop(sync_guard);

        println!(
            "Stopping synchronized test with {} workers",
            target_workers.len()
        );

        // Send synchronized stop message
        let stop_timestamp = Utc::now();
        let sync_stop_message = WebSocketMessage::SyncStop {
            timestamp: stop_timestamp,
            test_id: test_id.clone(),
            coordinator_id: self.config.coordinator_id.clone(),
            target_workers: target_workers.clone(),
            stop_timestamp,
        };

        self.message_sender.send(sync_stop_message)?;

        // Also send regular stop command
        let command_id = Uuid::new_v4().to_string();
        let stop_command = WebSocketMessage::TestCommand {
            timestamp: Utc::now(),
            command_id,
            command_type: TestCommandType::Stop,
            test_config: DistributedTestConfig {
                test_id: test_id.clone(),
                base_config: crate::integrations::websocket::TestConfig {
                    url: "".to_string(),
                    concurrent_requests: 0,
                    rps: None,
                    duration_secs: 0,
                    method: "GET".to_string(),
                    user_agent_mode: "default".to_string(),
                },
                worker_assignments: Vec::new(),
                coordination_settings: CoordinationSettings {
                    synchronized_start: false,
                    synchronized_stop: true,
                    sync_timeout_secs: 30,
                    max_sync_wait_secs: 60,
                    heartbeat_interval_secs: 10,
                    metrics_reporting_interval_secs: 5,
                    timeout_secs: 300,
                },
            },
            target_workers,
        };

        self.message_sender.send(stop_command)?;

        // Clean up sync tracker
        let mut sync_guard = self.sync_tracker.write().await;
        *sync_guard = None;

        let mut status_guard = self.test_status.write().await;
        *status_guard = CoordinatorTestStatus::Completed;

        println!("✅ Synchronized stop initiated for all workers");
        Ok(())
    }

    pub async fn get_connected_workers(&self) -> Vec<ConnectedWorker> {
        let workers_guard = self.workers.read().await;
        workers_guard.values().cloned().collect()
    }

    pub async fn get_test_status(&self) -> CoordinatorTestStatus {
        self.test_status.read().await.clone()
    }

    pub fn get_message_sender(&self) -> broadcast::Sender<WebSocketMessage> {
        self.message_sender.clone()
    }

    pub fn get_actual_port(&self) -> Option<u16> {
        self.actual_port
    }

    /// Get the distributed stats collector for external access
    pub fn get_distributed_stats(&self) -> Arc<DistributedStatsCollector> {
        Arc::clone(&self.distributed_stats)
    }

    /// Calculate optimal load distribution based on current worker state
    pub async fn calculate_load_distribution(
        &self,
        total_concurrent: usize,
        total_rps: Option<u64>,
    ) -> Result<LoadDistribution> {
        let workers = self.distributed_stats.get_all_workers().await;
        let load_balancer = self.load_balancer.read().await;
        let distribution =
            load_balancer.calculate_distribution(&workers, total_concurrent, total_rps);

        // Store the current distribution
        {
            let mut current_dist = self.current_distribution.write().await;
            *current_dist = Some(distribution.clone());
        }

        Ok(distribution)
    }

    /// Apply load distribution by sending worker assignments
    pub async fn apply_load_distribution(&self, distribution: &LoadDistribution) -> Result<()> {
        let mut successful_assignments = 0;
        let total_assignments = distribution.worker_allocations.len();

        for (worker_id, allocation) in &distribution.worker_allocations {
            // Create a distributed test config for this worker's allocation
            let worker_test_config = DistributedTestConfig {
                test_id: format!("load-balance-{}", chrono::Utc::now().timestamp()),
                base_config: crate::integrations::websocket::TestConfig {
                    url: "load-balanced".to_string(), // This would be set from the actual test config
                    concurrent_requests: allocation.concurrent_requests,
                    rps: allocation.rps_limit,
                    duration_secs: 0, // Continuous until stopped
                    method: "GET".to_string(),
                    user_agent_mode: "default".to_string(),
                },
                worker_assignments: vec![crate::integrations::websocket::WorkerAssignment {
                    worker_id: worker_id.clone(),
                    concurrent_requests: allocation.concurrent_requests,
                    rps: allocation.rps_limit,
                    duration_secs: None,
                    start_delay_secs: 0.0,
                }],
                coordination_settings: CoordinationSettings {
                    synchronized_start: false,
                    synchronized_stop: false,
                    sync_timeout_secs: 30,
                    max_sync_wait_secs: 60,
                    heartbeat_interval_secs: 10,
                    metrics_reporting_interval_secs: 5,
                    timeout_secs: 300,
                },
            };

            // Send load balancing command to specific worker
            let command_id = uuid::Uuid::new_v4().to_string();
            let load_balance_command = WebSocketMessage::TestCommand {
                timestamp: Utc::now(),
                command_id,
                command_type: TestCommandType::Start,
                test_config: worker_test_config,
                target_workers: vec![worker_id.clone()],
            };

            match self.message_sender.send(load_balance_command) {
                Ok(_) => {
                    successful_assignments += 1;
                    println!(
                        "Applied load allocation to {}: {} concurrent, {:?} RPS",
                        worker_id, allocation.concurrent_requests, allocation.rps_limit
                    );
                }
                Err(e) => {
                    eprintln!("Failed to send load allocation to {}: {}", worker_id, e);
                }
            }
        }

        if successful_assignments == total_assignments {
            println!(
                "✅ Load distribution applied successfully to all {} workers",
                total_assignments
            );
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "Only {}/{} worker allocations were applied successfully",
                successful_assignments,
                total_assignments
            ))
        }
    }

    /// Check if rebalancing is needed and perform it if necessary
    pub async fn check_and_rebalance(&self) -> Result<bool> {
        let current_distribution = self.current_distribution.read().await;
        let workers = self.distributed_stats.get_all_workers().await;
        let load_balancer = self.load_balancer.read().await;

        if let Some(ref distribution) = *current_distribution {
            if load_balancer.should_rebalance(distribution, &workers) {
                drop(current_distribution);
                drop(load_balancer);

                println!("🔄 Rebalancing load distribution...");

                // Recalculate distribution with current parameters
                // In a real implementation, these would come from the active test configuration
                let new_distribution = self.calculate_load_distribution(100, Some(200)).await?;
                self.apply_load_distribution(&new_distribution).await?;

                println!("✅ Load rebalancing completed");
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Get current load distribution
    pub async fn get_current_distribution(&self) -> Option<LoadDistribution> {
        self.current_distribution.read().await.clone()
    }

    /// Update load balancer configuration
    pub async fn update_load_balancer_config(&self, config: LoadDistributionConfig) {
        let mut load_balancer = self.load_balancer.write().await;
        load_balancer.update_config(config);
        println!("Updated load balancer configuration");
    }

    /// Get load balancer configuration
    pub async fn get_load_balancer_config(&self) -> LoadDistributionConfig {
        let load_balancer = self.load_balancer.read().await;
        load_balancer.get_config().clone()
    }

    pub async fn get_load_balancer(&self) -> Arc<RwLock<LoadDistributionEngine>> {
        Arc::clone(&self.load_balancer)
    }
}

async fn handle_worker_connection(
    stream: TcpStream,
    addr: SocketAddr,
    context: WorkerConnectionContext,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws_stream) => ws_stream,
        Err(e) => {
            eprintln!("WebSocket connection error from {}: {}", addr, e);
            return;
        }
    };

    println!("WebSocket connection established with {}", addr);

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();
    let mut message_receiver = context.message_sender.subscribe();

    // Handle incoming messages from worker
    let workers_clone = Arc::clone(&context.workers);
    let message_sender_clone = context.message_sender.clone();
    let coordinator_id_clone = context.coordinator_id.clone();
    let distributed_stats_clone = Arc::clone(&context.distributed_stats);
    let sync_tracker_clone = Arc::clone(&context.sync_tracker);
    let max_workers = context.max_workers;

    let receive_task = tokio::spawn(async move {
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(tokio_tungstenite::tungstenite::protocol::Message::Text(text)) => {
                    if let Ok(message) = serde_json::from_str::<WebSocketMessage>(&text) {
                        handle_worker_message(
                            message,
                            &workers_clone,
                            &message_sender_clone,
                            &coordinator_id_clone,
                            max_workers,
                            &distributed_stats_clone,
                            &sync_tracker_clone,
                        )
                        .await;
                    }
                }
                Ok(tokio_tungstenite::tungstenite::protocol::Message::Close(_)) => {
                    println!("Worker connection closed: {}", addr);
                    break;
                }
                Err(e) => {
                    eprintln!("WebSocket error from {}: {}", addr, e);
                    break;
                }
                _ => {}
            }
        }
    });

    // Handle outgoing messages to worker
    let send_task = tokio::spawn(async move {
        while let Ok(message) = message_receiver.recv().await {
            let json = match serde_json::to_string(&message) {
                Ok(json) => json,
                Err(e) => {
                    eprintln!("Failed to serialize message: {}", e);
                    continue;
                }
            };

            if let Err(e) = ws_sender
                .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                    json.into(),
                ))
                .await
            {
                eprintln!("Failed to send message to worker: {}", e);
                break;
            }
        }
    });

    // Wait for either task to complete
    tokio::select! {
        _ = receive_task => {},
        _ = send_task => {},
    }

    println!("Worker connection handler finished for {}", addr);
}

async fn handle_worker_message(
    message: WebSocketMessage,
    workers: &Arc<RwLock<HashMap<String, ConnectedWorker>>>,
    message_sender: &broadcast::Sender<WebSocketMessage>,
    coordinator_id: &str,
    max_workers: usize,
    distributed_stats: &Arc<DistributedStatsCollector>,
    sync_tracker: &Arc<RwLock<Option<SyncTracker>>>,
) {
    match message {
        WebSocketMessage::WorkerJoinRequest {
            worker_id,
            worker_info,
            ..
        } => {
            let mut workers_guard = workers.write().await;

            let (accepted, message_text) = if workers_guard.len() >= max_workers {
                (false, "Maximum number of workers reached".to_string())
            } else if workers_guard.contains_key(&worker_id) {
                (false, "Worker ID already exists".to_string())
            } else {
                let connection_time = Instant::now();
                workers_guard.insert(
                    worker_id.clone(),
                    ConnectedWorker {
                        worker_id: worker_id.clone(),
                        worker_info,
                        status: WorkerStatus::Idle,
                        last_heartbeat: connection_time,
                        current_load: WorkerLoad {
                            current_rps: 0.0,
                            active_connections: 0,
                            memory_usage_mb: 0,
                            cpu_usage_percent: 0.0,
                            total_requests_sent: 0,
                            errors_count: 0,
                        },
                        connection_time,
                    },
                );

                // Add worker to distributed stats tracking
                distributed_stats
                    .add_worker(worker_id.clone(), Utc::now())
                    .await;

                (true, "Worker successfully joined".to_string())
            };

            let response = WebSocketMessage::WorkerJoinResponse {
                timestamp: Utc::now(),
                worker_id,
                accepted,
                coordinator_id: coordinator_id.to_string(),
                message: message_text,
            };

            let _ = message_sender.send(response);
        }
        WebSocketMessage::WorkerHeartbeat {
            worker_id,
            status,
            current_load,
            ..
        } => {
            let mut workers_guard = workers.write().await;
            if let Some(worker) = workers_guard.get_mut(&worker_id) {
                worker.last_heartbeat = Instant::now();
                worker.status = status;
                worker.current_load = current_load;
            }
        }
        WebSocketMessage::TestCommandResponse { .. } => {
            // Forward the response to all listeners
            let _ = message_sender.send(message);
        }
        WebSocketMessage::WorkerMetrics {
            ref worker_id,
            ref metrics,
            ref worker_load,
            ..
        } => {
            // Update distributed stats with worker metrics
            distributed_stats
                .update_worker_metrics(
                    worker_id.clone(),
                    metrics.clone(),
                    worker_load.clone(),
                    "Running".to_string(),
                )
                .await;

            // Forward the metrics to all listeners
            let _ = message_sender.send(message);
        }
        WebSocketMessage::SyncReady {
            test_id,
            worker_id,
            ready_for_start,
            preparation_time_ms,
            ..
        } => {
            let mut sync_guard = sync_tracker.write().await;
            if let Some(ref mut tracker) = *sync_guard {
                if tracker.test_id == test_id && tracker.target_workers.contains(&worker_id) {
                    if ready_for_start {
                        tracker
                            .ready_workers
                            .insert(worker_id.clone(), Instant::now());
                        println!(
                            "✅ Worker {} ready for sync start ({}ms preparation time)",
                            worker_id, preparation_time_ms
                        );

                        // Check if all workers are now ready
                        if tracker.ready_workers.len() == tracker.target_workers.len() {
                            println!(
                                "🚀 All {} workers ready for synchronized start!",
                                tracker.target_workers.len()
                            );
                        }
                    } else {
                        // Worker reported not ready
                        println!("❌ Worker {} not ready for sync start", worker_id);

                        let status_message = WebSocketMessage::SyncStatus {
                            timestamp: Utc::now(),
                            test_id: test_id.clone(),
                            worker_id: worker_id.clone(),
                            sync_state: SyncState::Failed,
                            message: "Worker preparation failed".to_string(),
                        };
                        let _ = message_sender.send(status_message);
                    }
                }
            }
        }
        WebSocketMessage::SyncStatus {
            ref test_id,
            ref worker_id,
            ref sync_state,
            message: ref status_message,
            ..
        } => {
            println!(
                "📊 Sync status from {}: {:?} - {}",
                worker_id, sync_state, status_message
            );

            // Update sync tracker if applicable
            let mut sync_guard = sync_tracker.write().await;
            if let Some(ref mut tracker) = *sync_guard {
                if tracker.test_id == *test_id && tracker.target_workers.contains(worker_id) {
                    match sync_state {
                        SyncState::Ready => {
                            // Worker is ready for synchronized start
                            tracker
                                .ready_workers
                                .insert(worker_id.clone(), Instant::now());
                            println!("✅ Worker {} ready for synchronized start", worker_id);

                            // Check if all workers are now ready
                            if tracker.ready_workers.len() == tracker.target_workers.len() {
                                println!(
                                    "🚀 All {} workers ready for synchronized start!",
                                    tracker.target_workers.len()
                                );
                            }
                        }
                        SyncState::Running => {
                            println!("✅ Worker {} has started synchronized test", worker_id);
                        }
                        SyncState::Idle => {
                            println!("✅ Worker {} has stopped synchronized test", worker_id);
                        }
                        SyncState::Failed => {
                            println!("❌ Worker {} reported synchronization failure", worker_id);
                        }
                        _ => {
                            println!("📋 Worker {} sync state: {:?}", worker_id, sync_state);
                        }
                    }
                }
            }

            // Forward the sync status to all listeners
            let _ = message_sender.send(message);
        }
        _ => {
            // Handle other message types as needed
        }
    }
}
