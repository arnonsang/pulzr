use crate::integrations::websocket::{
    CoordinationSettings, DistributedTestConfig, TestConfig, WebSocketMessage, WorkerAssignment,
};
use anyhow::Result;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use std::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use uuid::Uuid;

/// Client for connecting to a distributed load testing coordinator
pub struct DistributedClient {
    coordinator_url: String,
}

impl DistributedClient {
    pub fn new(coordinator_host: &str, coordinator_port: u16) -> Self {
        Self {
            coordinator_url: format!("ws://{}:{}", coordinator_host, coordinator_port),
        }
    }

    /// Start a distributed load test
    pub async fn start_test(&self, config: DistributedTestConfig) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.coordinator_url).await?;
        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Send test start command
        let test_command = WebSocketMessage::TestCommand {
            timestamp: Utc::now(),
            command_id: Uuid::new_v4().to_string(),
            command_type: crate::integrations::websocket::TestCommandType::Start,
            test_config: config,
            target_workers: Vec::new(), // All workers
        };

        let json = serde_json::to_string(&test_command)?;
        ws_sender
            .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                json.into(),
            ))
            .await?;

        // Wait for acknowledgment with timeout
        let response = timeout(Duration::from_secs(10), ws_receiver.next()).await;
        match response {
            Ok(Some(Ok(tokio_tungstenite::tungstenite::protocol::Message::Text(text)))) => {
                if let Ok(message) = serde_json::from_str::<WebSocketMessage>(&text) {
                    match message {
                        WebSocketMessage::TestCommandResponse {
                            status, message, ..
                        } => {
                            println!("Test command response: {:?} - {}", status, message);
                        }
                        _ => {
                            println!("Unexpected response message");
                        }
                    }
                }
            }
            _ => {
                return Err(anyhow::anyhow!("Timeout waiting for test command response"));
            }
        }

        Ok(())
    }

    /// Stop a distributed load test
    pub async fn stop_test(&self) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.coordinator_url).await?;
        let (mut ws_sender, _) = ws_stream.split();

        let stop_command = WebSocketMessage::TestCommand {
            timestamp: Utc::now(),
            command_id: Uuid::new_v4().to_string(),
            command_type: crate::integrations::websocket::TestCommandType::Stop,
            test_config: DistributedTestConfig {
                test_id: "stop".to_string(),
                base_config: TestConfig {
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

        let json = serde_json::to_string(&stop_command)?;
        ws_sender
            .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                json.into(),
            ))
            .await?;

        println!("Stop command sent");
        Ok(())
    }

    /// Monitor distributed test progress
    pub async fn monitor_test(&self, duration_secs: u64) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.coordinator_url).await?;
        let (_, mut ws_receiver) = ws_stream.split();

        println!(
            "Monitoring distributed test for {} seconds...",
            duration_secs
        );

        let monitor_duration = Duration::from_secs(duration_secs);
        let monitor_timeout = timeout(monitor_duration, async {
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(tokio_tungstenite::tungstenite::protocol::Message::Text(text)) => {
                        if let Ok(message) = serde_json::from_str::<WebSocketMessage>(&text) {
                            match message {
                                WebSocketMessage::CoordinatorStatus {
                                    connected_workers,
                                    test_status,
                                    ..
                                } => {
                                    println!(
                                        "Coordinator status: {:?}, Workers: {}",
                                        test_status,
                                        connected_workers.len()
                                    );
                                }
                                WebSocketMessage::WorkerMetrics {
                                    worker_id, metrics, ..
                                } => {
                                    println!(
                                        "Worker {}: {} RPS, {} errors",
                                        worker_id, metrics.current_rps, metrics.requests_failed
                                    );
                                }
                                WebSocketMessage::TestCompleted { summary, .. } => {
                                    println!("Test completed: {:?}", summary);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::protocol::Message::Close(_)) => {
                        println!("Connection closed");
                        break;
                    }
                    Err(e) => {
                        eprintln!("WebSocket error: {}", e);
                        break;
                    }
                    _ => {}
                }
            }
        });

        match monitor_timeout.await {
            Ok(_) => println!("Monitoring completed"),
            Err(_) => println!("Monitoring timeout reached"),
        }

        Ok(())
    }
}

/// Helper function to create a simple distributed test configuration
pub fn create_simple_test_config(
    url: String,
    total_concurrent: usize,
    total_rps: Option<u64>,
    duration_secs: u64,
    worker_ids: Vec<String>,
) -> DistributedTestConfig {
    let workers_count = worker_ids.len();
    let concurrent_per_worker = if workers_count > 0 {
        total_concurrent / workers_count
    } else {
        total_concurrent
    };
    let rps_per_worker = total_rps.map(|rps| rps / workers_count as u64);

    let worker_assignments = worker_ids
        .into_iter()
        .enumerate()
        .map(|(i, worker_id)| WorkerAssignment {
            worker_id,
            concurrent_requests: concurrent_per_worker,
            rps: rps_per_worker,
            duration_secs: Some(duration_secs),
            start_delay_secs: i as f64 * 0.1, // Stagger start times slightly
        })
        .collect();

    DistributedTestConfig {
        test_id: Uuid::new_v4().to_string(),
        base_config: TestConfig {
            url,
            concurrent_requests: total_concurrent,
            rps: total_rps,
            duration_secs,
            method: "GET".to_string(),
            user_agent_mode: "default".to_string(),
        },
        worker_assignments,
        coordination_settings: CoordinationSettings {
            synchronized_start: true,
            synchronized_stop: true,
            sync_timeout_secs: 30,
            max_sync_wait_secs: 60,
            heartbeat_interval_secs: 10,
            metrics_reporting_interval_secs: 5,
            timeout_secs: 300,
        },
    }
}
