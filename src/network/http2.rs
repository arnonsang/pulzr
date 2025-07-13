use anyhow;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Http2Config {
    /// Enable HTTP/2 support
    pub enabled: bool,
    /// Force HTTP/2 prior knowledge (skip HTTP/1.1 upgrade)
    pub prior_knowledge: bool,
    /// Force HTTP/1.1 only (disable HTTP/2)
    pub http1_only: bool,
    /// HTTP/2 initial connection window size
    pub initial_connection_window_size: Option<u32>,
    /// HTTP/2 initial stream window size
    pub initial_stream_window_size: Option<u32>,
    /// HTTP/2 max frame size
    pub max_frame_size: Option<u32>,
    /// Enable HTTP/2 adaptive window sizing
    pub adaptive_window: bool,
    /// HTTP/2 keep alive interval
    pub keep_alive_interval: Option<u64>,
    /// HTTP/2 keep alive timeout
    pub keep_alive_timeout: Option<u64>,
}

impl Default for Http2Config {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default for compatibility
            prior_knowledge: false,
            http1_only: false,
            initial_connection_window_size: None, // Use reqwest defaults
            initial_stream_window_size: None,     // Use reqwest defaults
            max_frame_size: None,                 // Use reqwest defaults
            adaptive_window: true,                // Enable adaptive window by default
            keep_alive_interval: Some(30),        // 30 seconds
            keep_alive_timeout: Some(5),          // 5 seconds
        }
    }
}

impl Http2Config {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    pub fn with_prior_knowledge() -> Self {
        Self {
            enabled: true,
            prior_knowledge: true,
            ..Default::default()
        }
    }

    pub fn http1_only() -> Self {
        Self {
            enabled: false,
            http1_only: true,
            ..Default::default()
        }
    }

    pub fn with_window_sizes(connection_window: u32, stream_window: u32) -> Self {
        Self {
            enabled: true,
            initial_connection_window_size: Some(connection_window),
            initial_stream_window_size: Some(stream_window),
            ..Default::default()
        }
    }

    pub fn optimize_for_load_testing() -> Self {
        Self {
            enabled: true,
            prior_knowledge: false, // Allow negotiation for broader compatibility
            http1_only: false,
            initial_connection_window_size: Some(1048576), // 1MB
            initial_stream_window_size: Some(262144),      // 256KB
            max_frame_size: Some(32768),                   // 32KB
            adaptive_window: true,
            keep_alive_interval: Some(10), // More frequent for load testing
            keep_alive_timeout: Some(2),   // Shorter timeout
        }
    }

    pub fn validate(&self) -> Result<(), anyhow::Error> {
        if self.enabled && self.http1_only {
            return Err(anyhow::anyhow!(
                "Cannot enable both HTTP/2 and HTTP/1-only mode"
            ));
        }

        if let Some(window_size) = self.initial_connection_window_size {
            if window_size < 65535 {
                return Err(anyhow::anyhow!(
                    "HTTP/2 initial connection window size must be at least 65535 bytes"
                ));
            }
        }

        if let Some(window_size) = self.initial_stream_window_size {
            if window_size < 65535 {
                return Err(anyhow::anyhow!(
                    "HTTP/2 initial stream window size must be at least 65535 bytes"
                ));
            }
        }

        if let Some(frame_size) = self.max_frame_size {
            if !(16384..=16777215).contains(&frame_size) {
                return Err(anyhow::anyhow!(
                    "HTTP/2 max frame size must be between 16384 and 16777215 bytes"
                ));
            }
        }

        Ok(())
    }

    pub fn apply_to_client_builder(
        &self,
        builder: reqwest::ClientBuilder,
    ) -> reqwest::ClientBuilder {
        let mut builder = builder;

        if self.http1_only {
            // Force HTTP/1.1 only
            builder = builder.http1_only();
        } else if self.enabled {
            // Enable HTTP/2
            if self.prior_knowledge {
                builder = builder.http2_prior_knowledge();
            }

            if let Some(window_size) = self.initial_connection_window_size {
                builder = builder.http2_initial_connection_window_size(window_size);
            }

            if let Some(window_size) = self.initial_stream_window_size {
                builder = builder.http2_initial_stream_window_size(window_size);
            }

            if let Some(frame_size) = self.max_frame_size {
                builder = builder.http2_max_frame_size(frame_size);
            }

            if self.adaptive_window {
                builder = builder.http2_adaptive_window(true);
            }

            if let Some(interval) = self.keep_alive_interval {
                builder = builder
                    .http2_keep_alive_interval(Some(std::time::Duration::from_secs(interval)));
            }

            if let Some(timeout) = self.keep_alive_timeout {
                builder = builder.http2_keep_alive_timeout(std::time::Duration::from_secs(timeout));
            }

            // Enable HTTP/2 keep alive while idle
            builder = builder.http2_keep_alive_while_idle(true);
        }

        builder
    }

    pub fn get_protocol_info(&self) -> ProtocolInfo {
        ProtocolInfo {
            protocol: if self.http1_only {
                "HTTP/1.1".to_string()
            } else if self.enabled {
                if self.prior_knowledge {
                    "HTTP/2 (prior knowledge)".to_string()
                } else {
                    "HTTP/2 (with fallback)".to_string()
                }
            } else {
                "HTTP/1.1 (default)".to_string()
            },
            features: self.get_enabled_features(),
        }
    }

    fn get_enabled_features(&self) -> Vec<String> {
        let mut features = Vec::new();

        if self.enabled && !self.http1_only {
            features.push("HTTP/2 multiplexing".to_string());
            features.push("Header compression".to_string());
            features.push("Binary framing".to_string());

            if self.prior_knowledge {
                features.push("Prior knowledge".to_string());
            }

            if self.adaptive_window {
                features.push("Adaptive window".to_string());
            }

            if self.keep_alive_interval.is_some() {
                features.push("Keep-alive".to_string());
            }

            if let Some(window_size) = self.initial_connection_window_size {
                features.push(format!("Connection window: {} bytes", window_size));
            }

            if let Some(window_size) = self.initial_stream_window_size {
                features.push(format!("Stream window: {} bytes", window_size));
            }

            if let Some(frame_size) = self.max_frame_size {
                features.push(format!("Max frame: {} bytes", frame_size));
            }
        } else {
            features.push("Single connection per host".to_string());
            features.push("Text-based headers".to_string());
        }

        features
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolInfo {
    pub protocol: String,
    pub features: Vec<String>,
}

impl ProtocolInfo {
    pub fn print_summary(&self) {
        println!("🌐 Protocol Configuration:");
        println!("   Protocol: {}", self.protocol);
        println!("   Features:");
        for feature in &self.features {
            println!("     • {}", feature);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Http2Config::default();
        assert!(!config.enabled);
        assert!(!config.prior_knowledge);
        assert!(!config.http1_only);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_enabled_config() {
        let config = Http2Config::enabled();
        assert!(config.enabled);
        assert!(!config.prior_knowledge);
        assert!(!config.http1_only);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_prior_knowledge_config() {
        let config = Http2Config::with_prior_knowledge();
        assert!(config.enabled);
        assert!(config.prior_knowledge);
        assert!(!config.http1_only);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_http1_only_config() {
        let config = Http2Config::http1_only();
        assert!(!config.enabled);
        assert!(config.http1_only);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_load_testing_optimized() {
        let config = Http2Config::optimize_for_load_testing();
        assert!(config.enabled);
        assert!(config.adaptive_window);
        assert_eq!(config.keep_alive_interval, Some(10));
        assert_eq!(config.keep_alive_timeout, Some(2));
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_config() {
        let config = Http2Config {
            enabled: true,
            http1_only: true,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_window_size() {
        let config = Http2Config {
            initial_connection_window_size: Some(1000), // Too small
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_frame_size() {
        let config = Http2Config {
            max_frame_size: Some(1000), // Too small
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_protocol_info() {
        let config = Http2Config::enabled();
        let info = config.get_protocol_info();
        assert!(info.protocol.contains("HTTP/2"));
        assert!(!info.features.is_empty());
    }
}
