use crate::port_utils::find_available_port;
use crate::stats::{LiveMetrics, StatsCollector};
use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::{interval, Duration};
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::Message;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum WebSocketMessage {
    // Original WebUI messages
    #[serde(rename = "test_started")]
    TestStarted {
        timestamp: chrono::DateTime<chrono::Utc>,
        config: TestConfig,
    },
    #[serde(rename = "metrics_update")]
    MetricsUpdate {
        timestamp: chrono::DateTime<chrono::Utc>,
        metrics: LiveMetrics,
    },
    #[serde(rename = "request_log")]
    RequestLog {
        timestamp: chrono::DateTime<chrono::Utc>,
        log: crate::stats::RequestResult,
    },
    #[serde(rename = "test_completed")]
    TestCompleted {
        timestamp: chrono::DateTime<chrono::Utc>,
        summary: crate::stats::FinalSummary,
    },
    #[serde(rename = "error_event")]
    ErrorEvent {
        timestamp: chrono::DateTime<chrono::Utc>,
        error: String,
    },

    // Distributed load testing messages
    #[serde(rename = "worker_join_request")]
    WorkerJoinRequest {
        timestamp: chrono::DateTime<chrono::Utc>,
        worker_id: String,
        worker_info: WorkerInfo,
    },
    #[serde(rename = "worker_join_response")]
    WorkerJoinResponse {
        timestamp: chrono::DateTime<chrono::Utc>,
        worker_id: String,
        accepted: bool,
        coordinator_id: String,
        message: String,
    },
    #[serde(rename = "worker_heartbeat")]
    WorkerHeartbeat {
        timestamp: chrono::DateTime<chrono::Utc>,
        worker_id: String,
        status: WorkerStatus,
        current_load: WorkerLoad,
    },
    #[serde(rename = "test_command")]
    TestCommand {
        timestamp: chrono::DateTime<chrono::Utc>,
        command_id: String,
        command_type: TestCommandType,
        test_config: DistributedTestConfig,
        target_workers: Vec<String>, // Empty means all workers
    },
    #[serde(rename = "test_command_response")]
    TestCommandResponse {
        timestamp: chrono::DateTime<chrono::Utc>,
        command_id: String,
        worker_id: String,
        status: CommandResponseStatus,
        message: String,
    },
    #[serde(rename = "worker_metrics")]
    WorkerMetrics {
        timestamp: chrono::DateTime<chrono::Utc>,
        worker_id: String,
        metrics: LiveMetrics,
        worker_load: WorkerLoad,
    },
    #[serde(rename = "coordinator_status")]
    CoordinatorStatus {
        timestamp: chrono::DateTime<chrono::Utc>,
        coordinator_id: String,
        connected_workers: Vec<String>,
        test_status: CoordinatorTestStatus,
    },
    #[serde(rename = "worker_disconnect")]
    WorkerDisconnect {
        timestamp: chrono::DateTime<chrono::Utc>,
        worker_id: String,
        reason: String,
    },
    #[serde(rename = "worker_failure")]
    WorkerFailure {
        timestamp: chrono::DateTime<chrono::Utc>,
        worker_id: String,
        reason: String,
        last_seen: u64, // seconds since last heartbeat
        worker_info: WorkerInfo,
    },
    #[serde(rename = "worker_warning")]
    WorkerWarning {
        timestamp: chrono::DateTime<chrono::Utc>,
        worker_id: String,
        warning_type: String,
        message: String,
    },
    #[serde(rename = "load_rebalanced")]
    LoadRebalanced {
        timestamp: chrono::DateTime<chrono::Utc>,
        active_workers: Vec<String>,
        new_distribution: crate::distributed::load_balancer::LoadDistribution,
        reason: String,
    },
    #[serde(rename = "aggregated_metrics")]
    AggregatedMetrics {
        timestamp: chrono::DateTime<chrono::Utc>,
        aggregated_metrics: crate::metrics::distributed_stats::AggregatedMetrics,
    },

    // Synchronization messages for coordinated test execution
    #[serde(rename = "sync_prepare")]
    SyncPrepare {
        timestamp: chrono::DateTime<chrono::Utc>,
        test_id: String,
        coordinator_id: String,
        target_workers: Vec<String>,
        sync_timeout_secs: u64,
    },
    #[serde(rename = "sync_ready")]
    SyncReady {
        timestamp: chrono::DateTime<chrono::Utc>,
        test_id: String,
        worker_id: String,
        ready_for_start: bool,
        preparation_time_ms: u64,
    },
    #[serde(rename = "sync_start")]
    SyncStart {
        timestamp: chrono::DateTime<chrono::Utc>,
        test_id: String,
        coordinator_id: String,
        target_workers: Vec<String>,
        start_timestamp: chrono::DateTime<chrono::Utc>,
    },
    #[serde(rename = "sync_stop")]
    SyncStop {
        timestamp: chrono::DateTime<chrono::Utc>,
        test_id: String,
        coordinator_id: String,
        target_workers: Vec<String>,
        stop_timestamp: chrono::DateTime<chrono::Utc>,
    },
    #[serde(rename = "sync_status")]
    SyncStatus {
        timestamp: chrono::DateTime<chrono::Utc>,
        test_id: String,
        worker_id: String,
        sync_state: SyncState,
        message: String,
    },
    #[serde(rename = "sync_timeout")]
    SyncTimeout {
        timestamp: chrono::DateTime<chrono::Utc>,
        test_id: String,
        coordinator_id: String,
        timeout_reason: String,
        failed_workers: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestConfig {
    pub url: String,
    pub concurrent_requests: usize,
    pub rps: Option<u64>,
    pub duration_secs: u64,
    pub method: String,
    pub user_agent_mode: String,
}

// Distributed load testing data structures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerInfo {
    pub hostname: String,
    pub ip_address: String,
    pub port: u16,
    pub capabilities: WorkerCapabilities,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCapabilities {
    pub max_concurrent_requests: usize,
    pub max_rps: Option<u64>,
    pub supported_protocols: Vec<String>,
    pub available_memory_mb: u64,
    pub cpu_cores: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerStatus {
    Idle,
    Preparing,
    Running,
    Paused,
    Error,
    Disconnecting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerLoad {
    pub current_rps: f64,
    pub active_connections: usize,
    pub memory_usage_mb: u64,
    pub cpu_usage_percent: f64,
    pub total_requests_sent: u64,
    pub errors_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TestCommandType {
    Start,
    Stop,
    Pause,
    Resume,
    UpdateConfig,
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedTestConfig {
    pub test_id: String,
    pub base_config: TestConfig,
    pub worker_assignments: Vec<WorkerAssignment>,
    pub coordination_settings: CoordinationSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerAssignment {
    pub worker_id: String,
    pub concurrent_requests: usize,
    pub rps: Option<u64>,
    pub duration_secs: Option<u64>,
    pub start_delay_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinationSettings {
    pub synchronized_start: bool,
    pub synchronized_stop: bool,
    pub sync_timeout_secs: u64,
    pub max_sync_wait_secs: u64,
    pub heartbeat_interval_secs: u64,
    pub metrics_reporting_interval_secs: u64,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandResponseStatus {
    Acknowledged,
    Started,
    Completed,
    Failed,
    Rejected,
    PrepareReceived,
    ReadyToStart,
    SyncTimeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SyncState {
    Idle,
    Preparing,
    Ready,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    TimedOut,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoordinatorTestStatus {
    Idle,
    Preparing,
    Running,
    Paused,
    Stopping,
    Completed,
    Failed,
}

pub struct WebSocketServer {
    port: u16,
    actual_port: Option<u16>,
    stats_collector: Arc<StatsCollector>,
    message_sender: broadcast::Sender<WebSocketMessage>,
}

impl WebSocketServer {
    pub fn new(port: u16, stats_collector: Arc<StatsCollector>) -> Self {
        let (message_sender, _) = broadcast::channel(1000);

        Self {
            port,
            actual_port: None,
            stats_collector,
            message_sender,
        }
    }

    pub fn get_message_sender(&self) -> broadcast::Sender<WebSocketMessage> {
        self.message_sender.clone()
    }

    pub async fn start(&mut self) -> Result<u16> {
        let actual_port = find_available_port(self.port, 50).ok_or_else(|| {
            anyhow::anyhow!("Could not find available port starting from {}", self.port)
        })?;

        self.actual_port = Some(actual_port);

        let addr = SocketAddr::from(([127, 0, 0, 1], actual_port));
        let listener = TcpListener::bind(&addr).await?;

        if actual_port != self.port {
            println!(
                "WebSocket server listening on: ws://{} (auto-selected from preferred port {})",
                addr, self.port
            );
        } else {
            println!("WebSocket server listening on: ws://{addr}");
        }

        let stats_collector = Arc::clone(&self.stats_collector);
        let message_sender = self.message_sender.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));

            loop {
                interval.tick().await;
                let metrics = stats_collector.get_live_metrics().await;

                let message = WebSocketMessage::MetricsUpdate {
                    timestamp: chrono::Utc::now(),
                    metrics,
                };

                let _ = message_sender.send(message);
            }
        });

        let message_sender_clone = self.message_sender.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let message_receiver = message_sender_clone.subscribe();
                tokio::spawn(handle_connection(stream, message_receiver));
            }
        });

        Ok(actual_port)
    }

    pub fn send_test_started(&self, config: TestConfig) {
        let message = WebSocketMessage::TestStarted {
            timestamp: chrono::Utc::now(),
            config,
        };
        let _ = self.message_sender.send(message);
    }

    pub fn send_test_completed(&self, summary: crate::stats::FinalSummary) {
        let message = WebSocketMessage::TestCompleted {
            timestamp: chrono::Utc::now(),
            summary,
        };
        let _ = self.message_sender.send(message);
    }

    pub fn send_error(&self, error: String) {
        let message = WebSocketMessage::ErrorEvent {
            timestamp: chrono::Utc::now(),
            error,
        };
        let _ = self.message_sender.send(message);
    }

    pub fn get_actual_port(&self) -> Option<u16> {
        self.actual_port
    }
}

async fn handle_connection(
    stream: TcpStream,
    mut message_receiver: broadcast::Receiver<WebSocketMessage>,
) {
    let ws_stream = match accept_async(stream).await {
        Ok(ws_stream) => ws_stream,
        Err(e) => {
            eprintln!("WebSocket connection error: {e}");
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = ws_stream.split();

    let receive_task = tokio::spawn(async move {
        while let Some(msg) = ws_receiver.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    println!("Received: {text}");
                }
                Ok(Message::Close(_)) => {
                    println!("WebSocket connection closed");
                    break;
                }
                Err(e) => {
                    eprintln!("WebSocket error: {e}");
                    break;
                }
                _ => {}
            }
        }
    });

    let send_task = tokio::spawn(async move {
        while let Ok(message) = message_receiver.recv().await {
            let json = match serde_json::to_string(&message) {
                Ok(json) => json,
                Err(e) => {
                    eprintln!("Failed to serialize message: {e}");
                    continue;
                }
            };

            if let Err(e) = ws_sender.send(Message::Text(json.into())).await {
                eprintln!("Failed to send WebSocket message: {e}");
                break;
            }
        }
    });

    tokio::select! {
        _ = receive_task => {},
        _ = send_task => {},
    }
}
