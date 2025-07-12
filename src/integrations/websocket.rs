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
