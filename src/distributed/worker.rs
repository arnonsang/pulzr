use crate::core::load_tester::LoadTester;
use crate::integrations::websocket::{
    CommandResponseStatus, DistributedTestConfig, SyncState, TestCommandType, WebSocketMessage,
    WorkerCapabilities, WorkerInfo, WorkerLoad, WorkerStatus,
};
use crate::metrics::stats::StatsCollector;
use crate::utils::memory_info::get_system_memory_info;
use anyhow::Result;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::sync::{broadcast, RwLock};
use tokio::time::{interval, sleep, timeout};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct WorkerConfig {
    pub worker_id: String,
    pub coordinator_host: String,
    pub coordinator_port: u16,
    pub max_concurrent_requests: usize,
    pub max_rps: Option<u64>,
    pub heartbeat_interval_secs: u64,
    pub metrics_interval_secs: u64,
    pub connection_retry_interval_secs: u64,
    pub max_connection_retries: u32,
    pub connection_timeout_secs: u64,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            worker_id: format!("worker-{}", Uuid::new_v4()),
            coordinator_host: "localhost".to_string(),
            coordinator_port: 9630,
            max_concurrent_requests: 1000,
            max_rps: Some(10000),
            heartbeat_interval_secs: 10,
            metrics_interval_secs: 5,
            connection_retry_interval_secs: 5,
            max_connection_retries: 10,
            connection_timeout_secs: 30,
        }
    }
}

pub struct DistributedWorker {
    config: WorkerConfig,
    status: Arc<RwLock<WorkerStatus>>,
    current_load: Arc<RwLock<WorkerLoad>>,
    stats_collector: Arc<StatsCollector>,
    current_test: Arc<RwLock<Option<Arc<LoadTester>>>>,
    shutdown_sender: Option<broadcast::Sender<()>>,
}

impl DistributedWorker {
    pub fn new(config: WorkerConfig, stats_collector: Arc<StatsCollector>) -> Self {
        Self {
            config,
            status: Arc::new(RwLock::new(WorkerStatus::Idle)),
            current_load: Arc::new(RwLock::new(WorkerLoad {
                current_rps: 0.0,
                active_connections: 0,
                memory_usage_mb: 0,
                cpu_usage_percent: 0.0,
                total_requests_sent: 0,
                errors_count: 0,
            })),
            stats_collector,
            current_test: Arc::new(RwLock::new(None)),
            shutdown_sender: None,
        }
    }

    async fn connect_with_retry(
        &self,
        coordinator_url: &str,
    ) -> Result<WebSocketStream<MaybeTlsStream<TcpStream>>> {
        let mut retry_count = 0;
        let max_retries = self.config.max_connection_retries;
        let retry_interval = Duration::from_secs(self.config.connection_retry_interval_secs);
        let connection_timeout = Duration::from_secs(self.config.connection_timeout_secs);

        loop {
            match timeout(connection_timeout, connect_async(coordinator_url)).await {
                Ok(Ok((ws_stream, _))) => {
                    if retry_count > 0 {
                        println!(
                            "🔗 Reconnected to coordinator after {} attempts",
                            retry_count + 1
                        );
                    }
                    return Ok(ws_stream);
                }
                Ok(Err(e)) => {
                    retry_count += 1;
                    if retry_count > max_retries {
                        return Err(anyhow::anyhow!(
                            "Failed to connect to coordinator after {} attempts: {}",
                            max_retries,
                            e
                        ));
                    }

                    println!(
                        "⚠️  Connection attempt {} failed: {} (retrying in {}s)",
                        retry_count,
                        e,
                        retry_interval.as_secs()
                    );

                    sleep(retry_interval).await;
                }
                Err(_) => {
                    retry_count += 1;
                    if retry_count > max_retries {
                        return Err(anyhow::anyhow!(
                            "Connection timeout after {} attempts ({}s timeout)",
                            max_retries,
                            connection_timeout.as_secs()
                        ));
                    }

                    println!(
                        "⚠️  Connection attempt {} timed out after {}s (retrying in {}s)",
                        retry_count,
                        connection_timeout.as_secs(),
                        retry_interval.as_secs()
                    );

                    sleep(retry_interval).await;
                }
            }
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        println!("Starting distributed worker: {}", self.config.worker_id);
        println!(
            "Connecting to coordinator: {}:{}",
            self.config.coordinator_host, self.config.coordinator_port
        );

        let coordinator_url = format!(
            "ws://{}:{}",
            self.config.coordinator_host, self.config.coordinator_port
        );

        // Connection with retry logic
        let ws_stream = self.connect_with_retry(&coordinator_url).await?;
        println!("✅ Connected to coordinator successfully");

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();

        // Send join request
        let worker_info = self.get_worker_info().await?;
        let join_request = WebSocketMessage::WorkerJoinRequest {
            timestamp: Utc::now(),
            worker_id: self.config.worker_id.clone(),
            worker_info,
        };

        let join_json = serde_json::to_string(&join_request)?;
        ws_sender
            .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                join_json.into(),
            ))
            .await?;

        // Create shutdown channel
        let (shutdown_sender, mut shutdown_receiver) = broadcast::channel(1);
        self.shutdown_sender = Some(shutdown_sender);

        // We need to handle sending in the message loop instead of separate tasks
        // since SplitSink cannot be cloned

        // Start load monitoring task
        self.start_load_monitoring().await;

        // Handle incoming messages
        let status = Arc::clone(&self.status);
        let current_test = Arc::clone(&self.current_test);
        let stats_collector = Arc::clone(&self.stats_collector);
        let worker_id = self.config.worker_id.clone();

        let message_handler = tokio::spawn(async move {
            while let Some(msg) = ws_receiver.next().await {
                match msg {
                    Ok(tokio_tungstenite::tungstenite::protocol::Message::Text(text)) => {
                        if let Ok(message) = serde_json::from_str::<WebSocketMessage>(&text) {
                            handle_coordinator_message(
                                message,
                                &status,
                                &current_test,
                                &stats_collector,
                                &worker_id,
                                &mut ws_sender,
                            )
                            .await;
                        }
                    }
                    Ok(tokio_tungstenite::tungstenite::protocol::Message::Close(_)) => {
                        println!("Coordinator connection closed");
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

        // Wait for shutdown signal or connection loss
        tokio::select! {
            _ = message_handler => {
                println!("Connection to coordinator lost");
            }
            _ = shutdown_receiver.recv() => {
                println!("Shutdown signal received");
            }
        }

        Ok(())
    }

    async fn get_worker_info(&self) -> Result<WorkerInfo> {
        let hostname = hostname::get()
            .unwrap_or_else(|_| "unknown".into())
            .to_string_lossy()
            .to_string();

        let ip_address = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());

        let memory_info = get_system_memory_info().unwrap_or_default();
        let available_memory_mb = memory_info.total_mb as u64;

        let cpu_cores = num_cpus::get() as u32;

        let capabilities = WorkerCapabilities {
            max_concurrent_requests: self.config.max_concurrent_requests,
            max_rps: self.config.max_rps,
            supported_protocols: vec!["HTTP/1.1".to_string(), "HTTP/2".to_string()],
            available_memory_mb,
            cpu_cores,
        };

        Ok(WorkerInfo {
            hostname,
            ip_address,
            port: 0, // Worker doesn't listen on a port
            capabilities,
            version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    // TODO: Implement heartbeat and metrics in a more appropriate way
    // The current approach doesn't work because SplitSink cannot be cloned

    async fn start_load_monitoring(&self) {
        let current_load = Arc::clone(&self.current_load);
        let stats_collector = Arc::clone(&self.stats_collector);

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                let metrics = stats_collector.get_live_metrics().await;
                let memory_info = get_system_memory_info().unwrap_or_default();

                let mut load_guard = current_load.write().await;
                load_guard.current_rps = metrics.current_rps;
                load_guard.active_connections = metrics.requests_sent as usize;
                load_guard.memory_usage_mb = memory_info.used_mb as u64;
                load_guard.cpu_usage_percent = 0.0; // TODO: Implement CPU monitoring
                load_guard.total_requests_sent = metrics.requests_sent;
                load_guard.errors_count = metrics.requests_failed;
            }
        });
    }

    pub async fn shutdown(&self) {
        if let Some(sender) = &self.shutdown_sender {
            let _ = sender.send(());
        }

        // Stop current test if running
        let mut test_guard = self.current_test.write().await;
        if test_guard.is_some() {
            *test_guard = None;
        }

        let mut status_guard = self.status.write().await;
        *status_guard = WorkerStatus::Disconnecting;
    }

    pub async fn get_status(&self) -> WorkerStatus {
        self.status.read().await.clone()
    }
}

async fn handle_coordinator_message(
    message: WebSocketMessage,
    status: &Arc<RwLock<WorkerStatus>>,
    current_test: &Arc<RwLock<Option<Arc<LoadTester>>>>,
    stats_collector: &Arc<StatsCollector>,
    worker_id: &str,
    ws_sender: &mut futures::stream::SplitSink<
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        tokio_tungstenite::tungstenite::protocol::Message,
    >,
) {
    match message {
        WebSocketMessage::WorkerJoinResponse {
            accepted, message, ..
        } => {
            if accepted {
                println!("Successfully joined coordinator: {}", message);
                let mut status_guard = status.write().await;
                *status_guard = WorkerStatus::Idle;
            } else {
                eprintln!("Failed to join coordinator: {}", message);
                let mut status_guard = status.write().await;
                *status_guard = WorkerStatus::Error;
            }
        }
        WebSocketMessage::TestCommand {
            command_id,
            command_type,
            test_config,
            target_workers,
            ..
        } => {
            // Check if this command is for us
            if !target_workers.is_empty() && !target_workers.contains(&worker_id.to_string()) {
                return;
            }

            let response_status = match command_type {
                TestCommandType::Start => {
                    handle_start_command(
                        test_config,
                        status,
                        current_test,
                        stats_collector,
                        ws_sender,
                        worker_id,
                    )
                    .await
                }
                TestCommandType::Stop => handle_stop_command(status, current_test).await,
                TestCommandType::Pause => handle_pause_command(status).await,
                TestCommandType::Resume => handle_resume_command(status).await,
                _ => CommandResponseStatus::Rejected,
            };

            let response = WebSocketMessage::TestCommandResponse {
                timestamp: Utc::now(),
                command_id,
                worker_id: worker_id.to_string(),
                status: response_status,
                message: "Command processed".to_string(),
            };

            let json = match serde_json::to_string(&response) {
                Ok(json) => json,
                Err(e) => {
                    eprintln!("Failed to serialize response: {}", e);
                    return;
                }
            };

            if let Err(e) = ws_sender
                .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                    json.into(),
                ))
                .await
            {
                eprintln!("Failed to send command response: {}", e);
            }
        }
        WebSocketMessage::SyncStart {
            test_id,
            start_timestamp,
            ..
        } => {
            handle_sync_start(
                test_id,
                start_timestamp,
                status,
                current_test,
                ws_sender,
                worker_id,
            )
            .await;
        }
        WebSocketMessage::SyncStop {
            test_id,
            stop_timestamp,
            ..
        } => {
            handle_sync_stop(
                test_id,
                stop_timestamp,
                status,
                current_test,
                ws_sender,
                worker_id,
            )
            .await;
        }
        _ => {
            // Handle other message types as needed
        }
    }
}

async fn handle_start_command(
    test_config: DistributedTestConfig,
    status: &Arc<RwLock<WorkerStatus>>,
    _current_test: &Arc<RwLock<Option<Arc<LoadTester>>>>,
    _stats_collector: &Arc<StatsCollector>,
    ws_sender: &mut futures::stream::SplitSink<
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        tokio_tungstenite::tungstenite::protocol::Message,
    >,
    worker_id: &str,
) -> CommandResponseStatus {
    let mut status_guard = status.write().await;
    if !matches!(*status_guard, WorkerStatus::Idle) {
        return CommandResponseStatus::Rejected;
    }

    // Check if this is a synchronized test
    if test_config.coordination_settings.synchronized_start {
        *status_guard = WorkerStatus::Preparing;
        drop(status_guard);

        println!(
            "📋 Preparing for synchronized test: {}",
            test_config.test_id
        );

        // Send readiness signal to coordinator
        let ready_signal = WebSocketMessage::SyncStatus {
            timestamp: Utc::now(),
            test_id: test_config.test_id.clone(),
            worker_id: worker_id.to_string(),
            sync_state: SyncState::Ready,
            message: "Worker ready for synchronized start".to_string(),
        };

        if let Ok(json) = serde_json::to_string(&ready_signal) {
            let _ = ws_sender
                .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                    json.into(),
                ))
                .await;
        }

        println!(
            "✅ Ready for synchronized start of test: {}",
            test_config.test_id
        );
        CommandResponseStatus::Started
    } else {
        *status_guard = WorkerStatus::Running;
        drop(status_guard);

        // TODO: Create and start LoadTester with the distributed test config
        // For now, we'll acknowledge the command
        println!(
            "Starting distributed test (unsynchronized): {}",
            test_config.test_id
        );

        CommandResponseStatus::Started
    }
}

async fn handle_stop_command(
    status: &Arc<RwLock<WorkerStatus>>,
    current_test: &Arc<RwLock<Option<Arc<LoadTester>>>>,
) -> CommandResponseStatus {
    let mut status_guard = status.write().await;
    *status_guard = WorkerStatus::Idle;
    drop(status_guard);

    let mut test_guard = current_test.write().await;
    *test_guard = None;

    println!("Stopped distributed test");
    CommandResponseStatus::Completed
}

async fn handle_pause_command(status: &Arc<RwLock<WorkerStatus>>) -> CommandResponseStatus {
    let mut status_guard = status.write().await;
    if matches!(*status_guard, WorkerStatus::Running) {
        *status_guard = WorkerStatus::Paused;
        println!("Paused distributed test");
        CommandResponseStatus::Acknowledged
    } else {
        CommandResponseStatus::Rejected
    }
}

async fn handle_resume_command(status: &Arc<RwLock<WorkerStatus>>) -> CommandResponseStatus {
    let mut status_guard = status.write().await;
    if matches!(*status_guard, WorkerStatus::Paused) {
        *status_guard = WorkerStatus::Running;
        println!("Resumed distributed test");
        CommandResponseStatus::Acknowledged
    } else {
        CommandResponseStatus::Rejected
    }
}

async fn handle_sync_start(
    test_id: String,
    start_timestamp: chrono::DateTime<chrono::Utc>,
    status: &Arc<RwLock<WorkerStatus>>,
    _current_test: &Arc<RwLock<Option<Arc<LoadTester>>>>,
    ws_sender: &mut futures::stream::SplitSink<
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        tokio_tungstenite::tungstenite::protocol::Message,
    >,
    worker_id: &str,
) {
    println!(
        "🚀 Received synchronized start signal for test: {}",
        test_id
    );

    // Calculate delay until start time
    let now = Utc::now();
    let delay = start_timestamp.signed_duration_since(now);

    if delay.num_milliseconds() > 0 {
        println!(
            "⏱️  Waiting {}ms for synchronized start",
            delay.num_milliseconds()
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(
            delay.num_milliseconds() as u64
        ))
        .await;
    }

    // Update status to running
    {
        let mut status_guard = status.write().await;
        *status_guard = WorkerStatus::Running;
    }

    println!("✅ Started synchronized test: {}", test_id);

    // Send sync status update
    let sync_status = WebSocketMessage::SyncStatus {
        timestamp: Utc::now(),
        test_id,
        worker_id: worker_id.to_string(),
        sync_state: SyncState::Running,
        message: "Synchronized start completed".to_string(),
    };

    if let Ok(json) = serde_json::to_string(&sync_status) {
        let _ = ws_sender
            .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                json.into(),
            ))
            .await;
    }
}

async fn handle_sync_stop(
    test_id: String,
    stop_timestamp: chrono::DateTime<chrono::Utc>,
    status: &Arc<RwLock<WorkerStatus>>,
    _current_test: &Arc<RwLock<Option<Arc<LoadTester>>>>,
    ws_sender: &mut futures::stream::SplitSink<
        WebSocketStream<MaybeTlsStream<TcpStream>>,
        tokio_tungstenite::tungstenite::protocol::Message,
    >,
    worker_id: &str,
) {
    println!("🛑 Received synchronized stop signal for test: {}", test_id);

    // Calculate delay until stop time
    let now = Utc::now();
    let delay = stop_timestamp.signed_duration_since(now);

    if delay.num_milliseconds() > 0 {
        println!(
            "⏱️  Waiting {}ms for synchronized stop",
            delay.num_milliseconds()
        );
        tokio::time::sleep(tokio::time::Duration::from_millis(
            delay.num_milliseconds() as u64
        ))
        .await;
    }

    // Stop the current test
    {
        let mut status_guard = status.write().await;
        *status_guard = WorkerStatus::Idle;
    }

    {
        let mut test_guard = _current_test.write().await;
        *test_guard = None;
    }

    println!("✅ Stopped synchronized test: {}", test_id);

    // Send sync status update
    let sync_status = WebSocketMessage::SyncStatus {
        timestamp: Utc::now(),
        test_id,
        worker_id: worker_id.to_string(),
        sync_state: SyncState::Idle,
        message: "Synchronized stop completed".to_string(),
    };

    if let Ok(json) = serde_json::to_string(&sync_status) {
        let _ = ws_sender
            .send(tokio_tungstenite::tungstenite::protocol::Message::Text(
                json.into(),
            ))
            .await;
    }
}
