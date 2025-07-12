use crate::auth::AuthMethod;
use crate::network::Http2Config;
use crate::stats::{RequestResult, StatsCollector};
use crate::user_agent::UserAgentManager;
use anyhow::Result;
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use reqwest::{Client, Method};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct HttpClient {
    client: Client,
    url: String,
    method: Method,
    headers: HeaderMap,
    payload: Option<String>,
    user_agent_manager: Arc<UserAgentManager>,
    stats_collector: Arc<StatsCollector>,
    auth_method: Arc<AuthMethod>,
    http2_config: Arc<Http2Config>,
}

impl HttpClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        url: String,
        method: Method,
        headers: Vec<String>,
        payload: Option<String>,
        user_agent_manager: Arc<UserAgentManager>,
        stats_collector: Arc<StatsCollector>,
        timeout: Option<Duration>,
        auth_method: Arc<AuthMethod>,
        http2_config: Arc<Http2Config>,
    ) -> Result<Self> {
        let mut client_builder = Client::builder();

        if let Some(timeout) = timeout {
            client_builder = client_builder.timeout(timeout);
        }

        // Apply HTTP/2 configuration
        client_builder = http2_config.apply_to_client_builder(client_builder);

        let client = client_builder.build()?;

        let mut header_map = HeaderMap::new();
        for header in headers {
            if let Some((key, value)) = header.split_once(':') {
                let key = key.trim();
                let value = value.trim();
                if let (Ok(name), Ok(val)) = (
                    HeaderName::from_bytes(key.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    header_map.insert(name, val);
                }
            }
        }

        Ok(Self {
            client,
            url,
            method,
            headers: header_map,
            payload,
            user_agent_manager,
            stats_collector,
            auth_method,
            http2_config,
        })
    }

    pub async fn send_request(&self) -> Result<()> {
        let start = Instant::now();
        let timestamp = Utc::now();
        let user_agent = self.user_agent_manager.get_user_agent().to_string();

        let mut url = self.url.clone();

        // Handle query parameter authentication
        if let Some((key, value)) = self.auth_method.get_query_params().await {
            let separator = if url.contains('?') { "&" } else { "?" };
            url = format!("{url}{separator}{key}={value}");
        }

        let mut request_builder = self.client.request(self.method.clone(), &url);

        request_builder = request_builder.header("User-Agent", &user_agent);

        // Add authentication headers
        if let Some((key, value)) = self.auth_method.get_auth_header().await {
            request_builder = request_builder.header(key, value);
        }

        for (key, value) in &self.headers {
            request_builder = request_builder.header(key, value);
        }

        if let Some(payload) = &self.payload {
            if self.method == Method::POST
                || self.method == Method::PUT
                || self.method == Method::PATCH
            {
                request_builder = request_builder.body(payload.clone());

                if !self.headers.contains_key("content-type")
                    && (payload.trim_start().starts_with('{')
                        || payload.trim_start().starts_with('['))
                {
                    request_builder = request_builder.header("Content-Type", "application/json");
                }
            }
        }

        let request = request_builder.build()?;

        match self.client.execute(request).await {
            Ok(response) => {
                let status_code = response.status().as_u16();
                let content_length = response.content_length().unwrap_or(0);
                let duration = start.elapsed().as_millis() as u64;

                // Consider 4xx and 5xx status codes as errors
                let error = if status_code >= 400 {
                    Some(format!(
                        "HTTP {}: {}",
                        status_code,
                        response.status().canonical_reason().unwrap_or("Unknown")
                    ))
                } else {
                    None
                };

                let result = RequestResult {
                    timestamp,
                    duration_ms: duration,
                    status_code: Some(status_code),
                    error,
                    user_agent: Some(user_agent),
                    bytes_received: content_length,
                };

                self.stats_collector.record_request(result).await;
                Ok(())
            }
            Err(e) => {
                let duration = start.elapsed().as_millis() as u64;
                let result = RequestResult {
                    timestamp,
                    duration_ms: duration,
                    status_code: None,
                    error: Some(e.to_string()),
                    user_agent: Some(user_agent),
                    bytes_received: 0,
                };

                self.stats_collector.record_request(result).await;
                Err(e.into())
            }
        }
    }

    pub fn get_http2_config(&self) -> &Http2Config {
        &self.http2_config
    }

    pub fn get_protocol_info(&self) -> crate::network::ProtocolInfo {
        self.http2_config.get_protocol_info()
    }
}
