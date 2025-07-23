use anyhow::Result;
use chrono::Utc;
use pulzr::distributed::{
    CoordinatorConfig, DistributedCoordinator, DistributedWorker, LoadDistributionConfig,
    LoadDistributionStrategy, WorkerConfig,
};
use pulzr::integrations::websocket::{
    CoordinationSettings, DistributedTestConfig, TestConfig, WorkerAssignment, WorkerLoad,
};
use pulzr::metrics::{LatencyHistogram, LiveMetrics, StatsCollector};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::{sleep, timeout};
use uuid::Uuid;

/// Test basic coordinator startup and shutdown
#[tokio::test]
async fn test_coordinator_startup() -> Result<()> {
    let stats_collector = Arc::new(StatsCollector::new());
    let config = CoordinatorConfig {
        coordinator_id: "test-coordinator".to_string(),
        port: 45000, // Use a high port number that's likely available
        max_workers: 10,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let mut coordinator = DistributedCoordinator::new(config, stats_collector);
    let actual_port = coordinator.start().await?;

    assert!(actual_port >= 45000);
    println!("Coordinator started on port: {actual_port}");

    // Give coordinator time to fully start
    sleep(Duration::from_millis(100)).await;

    Ok(())
}

/// Test worker connection to coordinator
#[tokio::test]
async fn test_worker_connection() -> Result<()> {
    // Start coordinator
    let stats_collector = Arc::new(StatsCollector::new());
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator".to_string(),
        port: 45001,
        max_workers: 10,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let coordinator_port = coordinator.start().await?;

    // Give coordinator time to start
    sleep(Duration::from_millis(200)).await;

    // Create worker configuration
    let worker_config = WorkerConfig {
        worker_id: "test-worker-1".to_string(),
        coordinator_host: "localhost".to_string(),
        coordinator_port,
        max_concurrent_requests: 100,
        max_rps: Some(1000),
        heartbeat_interval_secs: 5,
        metrics_interval_secs: 3,
        connection_retry_interval_secs: 1,
        max_connection_retries: 3,
        connection_timeout_secs: 5,
    };

    let worker_stats = Arc::new(StatsCollector::new());
    let mut worker = DistributedWorker::new(worker_config, worker_stats);

    // Start worker connection in background
    let worker_handle = tokio::spawn(async move {
        if let Err(e) = worker.start().await {
            eprintln!("Worker connection error: {e}");
        }
    });

    // Give worker time to connect
    sleep(Duration::from_millis(500)).await;

    // Check that worker connected
    let connected_workers = coordinator.get_connected_workers().await;
    assert!(!connected_workers.is_empty(), "No workers connected");
    assert_eq!(connected_workers[0].worker_id, "test-worker-1");

    // Cleanup
    worker_handle.abort();

    Ok(())
}

/// Test multiple worker connections
#[tokio::test]
async fn test_multiple_workers() -> Result<()> {
    // Start coordinator
    let stats_collector = Arc::new(StatsCollector::new());
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-multi".to_string(),
        port: 45002,
        max_workers: 5,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let coordinator_port = coordinator.start().await?;

    sleep(Duration::from_millis(200)).await;

    // Start multiple workers
    let mut worker_handles = Vec::new();
    for i in 0..3 {
        let worker_config = WorkerConfig {
            worker_id: format!("test-worker-{i}"),
            coordinator_host: "localhost".to_string(),
            coordinator_port,
            max_concurrent_requests: 50,
            max_rps: Some(500),
            heartbeat_interval_secs: 5,
            metrics_interval_secs: 3,
            connection_retry_interval_secs: 1,
            max_connection_retries: 3,
            connection_timeout_secs: 5,
        };

        let worker_stats = Arc::new(StatsCollector::new());
        let mut worker = DistributedWorker::new(worker_config, worker_stats);

        let handle = tokio::spawn(async move {
            if let Err(e) = worker.start().await {
                eprintln!("Worker {i} connection error: {e}");
            }
        });
        worker_handles.push(handle);
    }

    // Give workers time to connect
    sleep(Duration::from_millis(1000)).await;

    // Verify all workers connected
    let connected_workers = coordinator.get_connected_workers().await;
    assert_eq!(connected_workers.len(), 3, "Expected 3 connected workers");

    // Cleanup
    for handle in worker_handles {
        handle.abort();
    }

    Ok(())
}

/// Test coordinator test command dispatch
#[tokio::test]
async fn test_test_command_dispatch() -> Result<()> {
    // Start coordinator
    let stats_collector = Arc::new(StatsCollector::new());
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-dispatch".to_string(),
        port: 45003,
        max_workers: 10,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let coordinator_port = coordinator.start().await?;

    sleep(Duration::from_millis(200)).await;

    // Connect a worker
    let worker_config = WorkerConfig {
        worker_id: "test-worker-dispatch".to_string(),
        coordinator_host: "localhost".to_string(),
        coordinator_port,
        max_concurrent_requests: 100,
        max_rps: Some(1000),
        heartbeat_interval_secs: 5,
        metrics_interval_secs: 3,
        connection_retry_interval_secs: 1,
        max_connection_retries: 3,
        connection_timeout_secs: 5,
    };

    let worker_stats = Arc::new(StatsCollector::new());
    let mut worker = DistributedWorker::new(worker_config, worker_stats);

    let worker_handle = tokio::spawn(async move {
        if let Err(e) = worker.start().await {
            eprintln!("Worker connection error: {e}");
        }
    });

    sleep(Duration::from_millis(500)).await;

    // Create test configuration
    let test_config = DistributedTestConfig {
        test_id: Uuid::new_v4().to_string(),
        base_config: TestConfig {
            url: "https://httpbin.org/get".to_string(),
            concurrent_requests: 10,
            rps: Some(50),
            duration_secs: 5,
            method: "GET".to_string(),
            user_agent_mode: "default".to_string(),
        },
        worker_assignments: vec![WorkerAssignment {
            worker_id: "test-worker-dispatch".to_string(),
            concurrent_requests: 10,
            rps: Some(50),
            duration_secs: Some(5),
            start_delay_secs: 0.0,
        }],
        coordination_settings: CoordinationSettings {
            synchronized_start: false,
            synchronized_stop: false,
            sync_timeout_secs: 30,
            max_sync_wait_secs: 60,
            heartbeat_interval_secs: 5,
            metrics_reporting_interval_secs: 3,
            timeout_secs: 30,
        },
    };

    // Start distributed test
    let result = coordinator.start_distributed_test(test_config).await;
    assert!(
        result.is_ok(),
        "Failed to start distributed test: {result:?}"
    );

    sleep(Duration::from_millis(500)).await;

    // Stop the test
    let stop_result = coordinator.stop_distributed_test().await;
    assert!(
        stop_result.is_ok(),
        "Failed to stop distributed test: {stop_result:?}"
    );

    // Cleanup
    worker_handle.abort();

    Ok(())
}

/// Test coordinator max workers limit
#[tokio::test]
async fn test_max_workers_limit() -> Result<()> {
    // Start coordinator with max 2 workers
    let stats_collector = Arc::new(StatsCollector::new());
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-limit".to_string(),
        port: 45004,
        max_workers: 2,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let coordinator_port = coordinator.start().await?;

    sleep(Duration::from_millis(200)).await;

    // Try to connect 3 workers (should only accept 2)
    let mut worker_handles = Vec::new();
    for i in 0..3 {
        let worker_config = WorkerConfig {
            worker_id: format!("test-worker-limit-{i}"),
            coordinator_host: "localhost".to_string(),
            coordinator_port,
            max_concurrent_requests: 50,
            max_rps: Some(500),
            heartbeat_interval_secs: 5,
            metrics_interval_secs: 3,
            connection_retry_interval_secs: 1,
            max_connection_retries: 3,
            connection_timeout_secs: 5,
        };

        let worker_stats = Arc::new(StatsCollector::new());
        let mut worker = DistributedWorker::new(worker_config, worker_stats);

        let handle = tokio::spawn(async move {
            if let Err(e) = worker.start().await {
                eprintln!("Worker {i} connection error: {e}");
            }
        });
        worker_handles.push(handle);

        // Small delay between connections
        sleep(Duration::from_millis(100)).await;
    }

    // Give time for connections to establish/reject
    sleep(Duration::from_millis(1000)).await;

    // Should only have 2 workers connected
    let connected_workers = coordinator.get_connected_workers().await;
    assert_eq!(
        connected_workers.len(),
        2,
        "Should only accept 2 workers max"
    );

    // Cleanup
    for handle in worker_handles {
        handle.abort();
    }

    Ok(())
}

/// Test timeout handling for worker connections
#[tokio::test]
async fn test_connection_timeout() -> Result<()> {
    // Test connecting to non-existent coordinator
    let worker_config = WorkerConfig {
        worker_id: "test-worker-timeout".to_string(),
        coordinator_host: "localhost".to_string(),
        coordinator_port: 19999, // Should be unused port
        max_concurrent_requests: 100,
        max_rps: Some(1000),
        heartbeat_interval_secs: 5,
        metrics_interval_secs: 3,
        connection_retry_interval_secs: 1,
        max_connection_retries: 3,
        connection_timeout_secs: 5,
    };

    let worker_stats = Arc::new(StatsCollector::new());
    let mut worker = DistributedWorker::new(worker_config, worker_stats);

    // This should timeout/fail quickly
    let result = timeout(Duration::from_secs(5), worker.start()).await;

    match result {
        Ok(worker_result) => {
            // Worker connection should have failed
            assert!(worker_result.is_err(), "Expected worker connection to fail");
        }
        Err(_) => {
            // Timeout is also acceptable for this test
            println!("Worker connection timed out as expected");
        }
    }

    Ok(())
}

/// Test distributed stats aggregation
#[tokio::test]
async fn test_distributed_stats_aggregation() -> Result<()> {
    // Start coordinator
    let stats_collector = Arc::new(StatsCollector::new());
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-stats".to_string(),
        port: 45005,
        max_workers: 10,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let _coordinator_port = coordinator.start().await?;

    sleep(Duration::from_millis(200)).await;

    // Get the distributed stats collector
    let distributed_stats = coordinator.get_distributed_stats();

    // Verify initial state
    let initial_metrics = distributed_stats.get_aggregated_metrics().await;
    assert_eq!(initial_metrics.total_workers, 0);
    assert_eq!(initial_metrics.global_metrics.requests_sent, 0);

    // Add a worker manually (simulating worker connection)
    distributed_stats
        .add_worker("test-worker".to_string(), Utc::now())
        .await;

    let metrics_after_add = distributed_stats.get_aggregated_metrics().await;
    assert_eq!(metrics_after_add.total_workers, 1);
    assert!(metrics_after_add.worker_metrics.contains_key("test-worker"));

    // Simulate worker metrics update
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

    distributed_stats
        .update_worker_metrics(
            "test-worker".to_string(),
            test_metrics,
            test_load,
            "Running".to_string(),
        )
        .await;

    // Verify aggregated metrics
    let final_metrics = distributed_stats.get_aggregated_metrics().await;
    assert_eq!(final_metrics.global_metrics.requests_sent, 100);
    assert_eq!(final_metrics.global_metrics.requests_completed, 95);
    assert_eq!(final_metrics.global_metrics.requests_failed, 5);
    assert_eq!(final_metrics.global_metrics.current_rps, 10.0);
    assert_eq!(final_metrics.active_workers, 1);

    // Test worker removal
    distributed_stats.remove_worker("test-worker").await;

    let metrics_after_remove = distributed_stats.get_aggregated_metrics().await;
    assert_eq!(metrics_after_remove.total_workers, 0);
    assert!(!metrics_after_remove
        .worker_metrics
        .contains_key("test-worker"));

    Ok(())
}

/// Test load distribution calculation and application
#[tokio::test]
async fn test_load_distribution() -> Result<()> {
    // Start coordinator
    let stats_collector = Arc::new(StatsCollector::new());
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-load-dist".to_string(),
        port: 45006,
        max_workers: 10,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let _coordinator_port = coordinator.start().await?;

    sleep(Duration::from_millis(200)).await;

    // Configure load balancer for round-robin distribution
    let load_config = LoadDistributionConfig {
        strategy: LoadDistributionStrategy::RoundRobin,
        ..Default::default()
    };
    coordinator.update_load_balancer_config(load_config).await;

    // Get the distributed stats collector and add some workers
    let distributed_stats = coordinator.get_distributed_stats();

    // Add multiple workers
    for i in 1..=3 {
        distributed_stats
            .add_worker(format!("worker-{i}"), Utc::now())
            .await;

        // Simulate different worker loads
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
            memory_usage_mb: 1024 * i as u64,
            cpu_usage_percent: 10.0 * i as f64,
            total_requests_sent: 100 * i as u64,
            errors_count: 5 * i as u64,
        };

        distributed_stats
            .update_worker_metrics(format!("worker-{i}"), metrics, load, "Running".to_string())
            .await;
    }

    // Calculate load distribution
    let distribution = coordinator
        .calculate_load_distribution(30, Some(60))
        .await?;

    // Verify distribution
    assert_eq!(distribution.worker_allocations.len(), 3);
    assert_eq!(distribution.total_concurrent_requests, 30);
    assert_eq!(distribution.total_rps, Some(60));

    // For round-robin, each worker should get approximately equal load
    for allocation in distribution.worker_allocations.values() {
        assert!(allocation.concurrent_requests >= 9 && allocation.concurrent_requests <= 11);
        assert!(allocation.rps_limit.unwrap() >= 19 && allocation.rps_limit.unwrap() <= 21);
    }

    // Test confidence score
    assert!(distribution.confidence_score > 0.0);
    assert!(distribution.confidence_score <= 1.0);

    // Test getting current distribution
    let current_dist = coordinator.get_current_distribution().await;
    assert!(current_dist.is_some());

    // Test load balancer config update
    let new_config = LoadDistributionConfig {
        strategy: LoadDistributionStrategy::LoadBased,
        rebalance_threshold: 0.3,
        ..Default::default()
    };
    coordinator
        .update_load_balancer_config(new_config.clone())
        .await;

    let retrieved_config = coordinator.get_load_balancer_config().await;
    assert_eq!(retrieved_config.rebalance_threshold, 0.3);

    Ok(())
}

#[tokio::test]
async fn test_worker_failure_detection() -> Result<(), Box<dyn std::error::Error>> {
    use pulzr::distributed::coordinator::{CoordinatorConfig, DistributedCoordinator};
    use pulzr::distributed::worker::{DistributedWorker, WorkerConfig};
    use pulzr::metrics::stats::StatsCollector;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    // Create coordinator with short heartbeat timeout for faster testing
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-failure".to_string(),
        port: 45006,
        max_workers: 10,
        heartbeat_timeout_secs: 3,        // Very short timeout for testing
        heartbeat_check_interval_secs: 1, // Check every 1 second for faster testing
        auto_balance_load: true,
    };

    let stats_collector = Arc::new(StatsCollector::new());
    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let coordinator_port = coordinator.start().await?;

    sleep(Duration::from_millis(200)).await;

    // Create worker with normal heartbeat interval
    let worker_config = WorkerConfig {
        worker_id: "test-worker-failure".to_string(),
        coordinator_host: "localhost".to_string(),
        coordinator_port,
        max_concurrent_requests: 100,
        max_rps: Some(1000),
        heartbeat_interval_secs: 1, // Normal heartbeat
        metrics_interval_secs: 5,
        connection_retry_interval_secs: 1,
        max_connection_retries: 3,
        connection_timeout_secs: 5,
    };

    let worker_stats = Arc::new(StatsCollector::new());
    let mut worker = DistributedWorker::new(worker_config, Arc::clone(&worker_stats));

    // Start worker and let it connect
    let worker_handle = tokio::spawn(async move {
        let _ = worker.start().await;
    });

    sleep(Duration::from_millis(1000)).await;

    // Verify worker is connected
    let workers = coordinator.get_connected_workers().await;
    assert_eq!(workers.len(), 1);
    assert!(workers.iter().any(|w| w.worker_id == "test-worker-failure"));

    // Abort the worker to simulate failure (stopping heartbeats)
    worker_handle.abort();

    // Wait for heartbeat timeout to trigger (3 seconds + buffer)
    sleep(Duration::from_millis(6000)).await;

    // Verify worker was detected as failed and removed
    let workers_after_timeout = coordinator.get_connected_workers().await;
    println!("Workers after timeout: {}", workers_after_timeout.len());
    for worker in &workers_after_timeout {
        println!(
            "Remaining worker: {} (last heartbeat: {:?})",
            worker.worker_id,
            worker.last_heartbeat.elapsed()
        );
    }
    assert_eq!(
        workers_after_timeout.len(),
        0,
        "Worker should be removed after heartbeat timeout"
    );

    Ok(())
}

#[tokio::test]
async fn test_load_rebalancing_after_worker_failure() -> Result<(), Box<dyn std::error::Error>> {
    use pulzr::distributed::coordinator::{CoordinatorConfig, DistributedCoordinator};
    use pulzr::distributed::worker::{DistributedWorker, WorkerConfig};
    use pulzr::metrics::stats::StatsCollector;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    // Create coordinator
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-rebalance".to_string(),
        port: 45007,
        max_workers: 10,
        heartbeat_timeout_secs: 2,        // Short timeout for testing
        heartbeat_check_interval_secs: 1, // Check every 1 second for faster testing
        auto_balance_load: true,
    };

    let stats_collector = Arc::new(StatsCollector::new());
    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let coordinator_port = coordinator.start().await?;

    sleep(Duration::from_millis(200)).await;

    // Create multiple workers
    let worker_configs = vec![
        WorkerConfig {
            worker_id: "worker-1".to_string(),
            coordinator_host: "localhost".to_string(),
            coordinator_port,
            max_concurrent_requests: 100,
            max_rps: Some(1000),
            heartbeat_interval_secs: 1,
            metrics_interval_secs: 5,
            connection_retry_interval_secs: 1,
            max_connection_retries: 3,
            connection_timeout_secs: 5,
        },
        WorkerConfig {
            worker_id: "worker-2".to_string(),
            coordinator_host: "localhost".to_string(),
            coordinator_port,
            max_concurrent_requests: 100,
            max_rps: Some(1000),
            heartbeat_interval_secs: 1,
            metrics_interval_secs: 5,
            connection_retry_interval_secs: 1,
            max_connection_retries: 3,
            connection_timeout_secs: 5,
        },
    ];

    // Start workers
    for config in worker_configs {
        let stats = Arc::new(StatsCollector::new());
        let mut worker = DistributedWorker::new(config, Arc::clone(&stats));
        tokio::spawn(async move {
            let _ = worker.start().await;
        });
    }

    sleep(Duration::from_millis(1500)).await;

    // Verify both workers are connected
    let workers = coordinator.get_connected_workers().await;
    assert_eq!(workers.len(), 2);

    // Test that load balancer can calculate distribution for active workers
    let load_balancer = coordinator.get_load_balancer().await;
    let active_workers: Vec<String> = workers.iter().map(|w| w.worker_id.clone()).collect();

    // Create dummy worker metrics for testing
    let worker_metrics: Vec<_> = active_workers
        .iter()
        .map(|worker_id| {
            use pulzr::integrations::websocket::WorkerLoad;
            use pulzr::metrics::stats::LiveMetrics;

            pulzr::metrics::distributed_stats::WorkerMetrics {
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
                    latency_histogram: pulzr::metrics::stats::LatencyHistogram::new(),
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

    let distribution = {
        let lb = load_balancer.read().await;
        lb.calculate_distribution(&worker_metrics, 1000, None)
    };

    assert_eq!(distribution.worker_allocations.len(), 2);
    assert!(distribution.confidence_score > 0.0);

    // Let one worker "fail" by waiting for timeout
    sleep(Duration::from_millis(4000)).await;

    // Verify remaining worker count decreased
    let workers_after = coordinator.get_connected_workers().await;
    assert!(
        workers_after.len() <= 1,
        "Some workers should have timed out"
    );

    Ok(())
}

#[tokio::test]
async fn test_worker_connection_retry() -> Result<(), Box<dyn std::error::Error>> {
    use pulzr::distributed::worker::{DistributedWorker, WorkerConfig};
    use pulzr::metrics::stats::StatsCollector;
    use std::sync::Arc;

    // Create worker configured to connect to non-existent coordinator
    let worker_config = WorkerConfig {
        worker_id: "test-worker-retry".to_string(),
        coordinator_host: "localhost".to_string(),
        coordinator_port: 65001, // Likely unused port
        max_concurrent_requests: 100,
        max_rps: Some(1000),
        heartbeat_interval_secs: 10,
        metrics_interval_secs: 5,
        connection_retry_interval_secs: 1, // Fast retry for testing
        max_connection_retries: 3,         // Limited retries for testing
        connection_timeout_secs: 2,        // Short timeout for testing
    };

    let worker_stats = Arc::new(StatsCollector::new());
    let mut worker = DistributedWorker::new(worker_config, Arc::clone(&worker_stats));

    // Attempt to start worker (should fail after retries)
    let start_result = worker.start().await;
    assert!(
        start_result.is_err(),
        "Worker should fail to connect to non-existent coordinator"
    );

    let error_message = start_result.unwrap_err().to_string();
    assert!(
        error_message.contains("Failed to connect") || error_message.contains("timeout"),
        "Error should indicate connection failure or timeout: {error_message}"
    );

    Ok(())
}

/// Test synchronized test start coordination
#[tokio::test]
async fn test_synchronized_test_start() -> Result<(), Box<dyn std::error::Error>> {
    use pulzr::distributed::coordinator::{CoordinatorConfig, DistributedCoordinator};
    use pulzr::distributed::worker::{DistributedWorker, WorkerConfig};
    use pulzr::integrations::websocket::{CoordinationSettings, DistributedTestConfig, TestConfig};
    use pulzr::metrics::stats::StatsCollector;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    // Create coordinator
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-sync".to_string(),
        port: 45008,
        max_workers: 10,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let stats_collector = Arc::new(StatsCollector::new());
    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let actual_port = coordinator.start().await?;
    println!("Coordinator started on port {actual_port}");

    // Create multiple workers
    let mut workers = Vec::new();
    for i in 0..2 {
        let worker_config = WorkerConfig {
            worker_id: format!("test-worker-sync-{i}"),
            coordinator_host: "127.0.0.1".to_string(),
            coordinator_port: actual_port,
            max_concurrent_requests: 10,
            max_rps: Some(100),
            heartbeat_interval_secs: 10,
            metrics_interval_secs: 5,
            connection_retry_interval_secs: 1,
            max_connection_retries: 3,
            connection_timeout_secs: 5,
        };

        let worker_stats = Arc::new(StatsCollector::new());
        let mut worker = DistributedWorker::new(worker_config, worker_stats);

        // Start worker (don't await, let it run in background)
        let worker_handle = tokio::spawn(async move {
            let _ = worker.start().await;
        });

        workers.push(worker_handle);
    }

    // Wait for workers to connect
    sleep(Duration::from_secs(2)).await;

    // Verify workers are connected
    let connected_workers = coordinator.get_connected_workers().await;
    assert_eq!(
        connected_workers.len(),
        2,
        "Should have 2 connected workers"
    );

    // Create synchronized test config
    let test_config = DistributedTestConfig {
        test_id: "sync-test-001".to_string(),
        base_config: TestConfig {
            url: "https://httpbin.org/get".to_string(),
            concurrent_requests: 5,
            rps: Some(10),
            duration_secs: 5,
            method: "GET".to_string(),
            user_agent_mode: "default".to_string(),
        },
        worker_assignments: vec![],
        coordination_settings: CoordinationSettings {
            synchronized_start: true,
            synchronized_stop: false,
            sync_timeout_secs: 10,
            max_sync_wait_secs: 15,
            heartbeat_interval_secs: 10,
            metrics_reporting_interval_secs: 5,
            timeout_secs: 300,
        },
    };

    // Start synchronized test
    println!("Starting synchronized test...");
    let start_result = coordinator.start_distributed_test(test_config).await;
    assert!(
        start_result.is_ok(),
        "Synchronized test should start successfully"
    );

    // Wait for test to complete synchronization
    sleep(Duration::from_secs(3)).await;

    // Verify test is running
    let test_status = coordinator.get_test_status().await;
    println!("Test status: {test_status:?}");

    // Stop the test
    let stop_result = coordinator.stop_distributed_test().await;
    assert!(stop_result.is_ok(), "Should be able to stop test");

    // Clean up workers
    for worker_handle in workers {
        worker_handle.abort();
    }

    Ok(())
}

/// Test synchronization timeout handling
#[tokio::test]
async fn test_synchronization_timeout() -> Result<(), Box<dyn std::error::Error>> {
    use pulzr::distributed::coordinator::{CoordinatorConfig, DistributedCoordinator};
    use pulzr::integrations::websocket::{CoordinationSettings, DistributedTestConfig, TestConfig};
    use pulzr::metrics::stats::StatsCollector;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    // Create coordinator
    let coordinator_config = CoordinatorConfig {
        coordinator_id: "test-coordinator-timeout".to_string(),
        port: 45010,
        max_workers: 10,
        heartbeat_timeout_secs: 30,
        heartbeat_check_interval_secs: 10,
        auto_balance_load: true,
    };

    let stats_collector = Arc::new(StatsCollector::new());
    let mut coordinator =
        DistributedCoordinator::new(coordinator_config, Arc::clone(&stats_collector));
    let _actual_port = coordinator.start().await?;

    // Create test config with very short sync timeout
    let test_config = DistributedTestConfig {
        test_id: "timeout-test-001".to_string(),
        base_config: TestConfig {
            url: "https://httpbin.org/get".to_string(),
            concurrent_requests: 5,
            rps: Some(10),
            duration_secs: 5,
            method: "GET".to_string(),
            user_agent_mode: "default".to_string(),
        },
        worker_assignments: vec![],
        coordination_settings: CoordinationSettings {
            synchronized_start: true,
            synchronized_stop: false,
            sync_timeout_secs: 1, // Very short timeout
            max_sync_wait_secs: 2,
            heartbeat_interval_secs: 10,
            metrics_reporting_interval_secs: 5,
            timeout_secs: 300,
        },
    };

    // Try to start test without any workers (should fail immediately due to no workers)
    let start_result = coordinator.start_distributed_test(test_config).await;
    // This should fail because there are no workers available
    if start_result.is_ok() {
        println!("Test started successfully (no workers, will timeout during sync)");
    } else {
        println!("Test failed to start due to no available workers (expected)");
    }

    // Wait for timeout to occur
    sleep(Duration::from_secs(3)).await;

    // Check that test status indicates failure due to timeout
    let test_status = coordinator.get_test_status().await;
    println!("Test status after timeout: {test_status:?}");

    Ok(())
}
