use chrono::{DateTime, Utc};
use reqwest::{Request, Response};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct DebugConfig {
    pub enabled: bool,
    pub level: DebugLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DebugLevel {
    Basic = 1,   // Basic request/response info
    Headers = 2, // Include headers
    Full = 3,    // Include body content
}

impl From<u8> for DebugLevel {
    fn from(level: u8) -> Self {
        match level {
            1 => DebugLevel::Basic,
            2 => DebugLevel::Headers,
            3 => DebugLevel::Full,
            _ => DebugLevel::Basic,
        }
    }
}

#[derive(Debug)]
pub struct RequestDebugInfo {
    pub timestamp: DateTime<Utc>,
    pub method: String,
    pub url: String,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug)]
pub struct ResponseDebugInfo {
    pub timestamp: DateTime<Utc>,
    pub status: u16,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
    pub content_length: Option<u64>,
    pub duration: Duration,
}

#[derive(Debug)]
pub struct DebugSession {
    pub request: RequestDebugInfo,
    pub response: Option<ResponseDebugInfo>,
    pub error: Option<String>,
    pub start_time: Instant,
}

impl DebugConfig {
    pub fn new(enabled: bool, level: u8) -> Self {
        Self {
            enabled,
            level: DebugLevel::from(level),
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            level: DebugLevel::Basic,
        }
    }

    /// Log request information based on debug level
    pub fn log_request(&self, request_info: &RequestDebugInfo, session_id: &str) {
        if !self.enabled {
            return;
        }

        println!(
            "🔍 [DEBUG] Request {} [{}]",
            session_id,
            request_info.timestamp.format("%H:%M:%S%.3f")
        );
        println!("  {} {}", request_info.method, request_info.url);

        if let Some(ua) = &request_info.user_agent {
            println!("  User-Agent: {}", ua);
        }

        if self.level >= DebugLevel::Headers {
            if let Some(headers) = &request_info.headers {
                println!("  Headers:");
                for (key, value) in headers {
                    println!("    {}: {}", key, value);
                }
            }
        }

        if self.level >= DebugLevel::Full {
            if let Some(body) = &request_info.body {
                println!("  Body: {}", body);
            }
        }
    }

    /// Log response information based on debug level
    pub fn log_response(&self, response_info: &ResponseDebugInfo, session_id: &str) {
        if !self.enabled {
            return;
        }

        println!(
            "📨 [DEBUG] Response {} [{}]",
            session_id,
            response_info.timestamp.format("%H:%M:%S%.3f")
        );
        println!(
            "  Status: {} ({:?})",
            response_info.status, response_info.duration
        );

        if let Some(content_length) = response_info.content_length {
            println!("  Content-Length: {} bytes", content_length);
        }

        if self.level >= DebugLevel::Headers {
            if let Some(headers) = &response_info.headers {
                println!("  Headers:");
                for (key, value) in headers {
                    println!("    {}: {}", key, value);
                }
            }
        }

        if self.level >= DebugLevel::Full {
            if let Some(body) = &response_info.body {
                let preview = if body.len() > 500 {
                    format!("{}... ({} chars total)", &body[..500], body.len())
                } else {
                    body.clone()
                };
                println!("  Body: {}", preview);
            }
        }
    }

    /// Log error information
    pub fn log_error(&self, error: &str, session_id: &str, duration: Duration) {
        if !self.enabled {
            return;
        }

        println!("❌ [DEBUG] Error {} ({:?})", session_id, duration);
        println!("  {}", error);
    }

    /// Create a debug session for tracking request/response
    pub fn start_session(&self, method: &str, url: &str) -> Option<DebugSession> {
        if !self.enabled {
            return None;
        }

        let request_info = RequestDebugInfo {
            timestamp: Utc::now(),
            method: method.to_string(),
            url: url.to_string(),
            headers: None,
            body: None,
            user_agent: None,
        };

        Some(DebugSession {
            request: request_info,
            response: None,
            error: None,
            start_time: Instant::now(),
        })
    }

    /// Generate a unique session ID for correlation
    pub fn generate_session_id() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        format!("{:06}", COUNTER.fetch_add(1, Ordering::SeqCst))
    }
}

/// Helper functions for extracting debug information from HTTP types
impl RequestDebugInfo {
    pub fn from_reqwest_request(
        request: &Request,
        include_headers: bool,
        include_body: bool,
    ) -> Self {
        let mut headers = None;
        let mut user_agent = None;

        if include_headers {
            let mut header_map = HashMap::new();
            for (name, value) in request.headers() {
                if let Ok(value_str) = value.to_str() {
                    let name_str = name.as_str();
                    if name_str.to_lowercase() == "user-agent" {
                        user_agent = Some(value_str.to_string());
                    }
                    header_map.insert(name_str.to_string(), value_str.to_string());
                }
            }
            if !header_map.is_empty() {
                headers = Some(header_map);
            }
        } else {
            // Extract just user-agent even if not including all headers
            if let Some(ua_value) = request.headers().get("user-agent") {
                if let Ok(ua_str) = ua_value.to_str() {
                    user_agent = Some(ua_str.to_string());
                }
            }
        }

        let body = if include_body {
            // Note: reqwest::Request doesn't easily expose body content
            // This would need to be captured before sending
            None
        } else {
            None
        };

        Self {
            timestamp: Utc::now(),
            method: request.method().to_string(),
            url: request.url().to_string(),
            headers,
            body,
            user_agent,
        }
    }
}

impl ResponseDebugInfo {
    pub async fn from_reqwest_response(
        response: Response,
        include_headers: bool,
        include_body: bool,
        duration: Duration,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let status = response.status().as_u16();
        let content_length = response.content_length();

        let mut headers = None;
        if include_headers {
            let mut header_map = HashMap::new();
            for (name, value) in response.headers() {
                if let Ok(value_str) = value.to_str() {
                    header_map.insert(name.as_str().to_string(), value_str.to_string());
                }
            }
            if !header_map.is_empty() {
                headers = Some(header_map);
            }
        }

        let body = if include_body {
            // Extract body for debugging
            let bytes = response.bytes().await?;
            let body_text = String::from_utf8_lossy(&bytes).to_string();
            Some(body_text)
        } else {
            None
        };

        let debug_info = Self {
            timestamp: Utc::now(),
            status,
            headers,
            body,
            content_length,
            duration,
        };

        Ok(debug_info)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_level_conversion() {
        assert_eq!(DebugLevel::from(1), DebugLevel::Basic);
        assert_eq!(DebugLevel::from(2), DebugLevel::Headers);
        assert_eq!(DebugLevel::from(3), DebugLevel::Full);
        assert_eq!(DebugLevel::from(99), DebugLevel::Basic); // Default fallback
    }

    #[test]
    fn test_debug_config() {
        let config = DebugConfig::new(true, 2);
        assert!(config.enabled);
        assert_eq!(config.level, DebugLevel::Headers);

        let disabled = DebugConfig::disabled();
        assert!(!disabled.enabled);
        assert_eq!(disabled.level, DebugLevel::Basic);
    }

    #[test]
    fn test_session_id_generation() {
        let id1 = DebugConfig::generate_session_id();
        let id2 = DebugConfig::generate_session_id();
        assert_ne!(id1, id2);
        assert_eq!(id1.len(), 6);
        assert_eq!(id2.len(), 6);
    }
}
