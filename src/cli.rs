use clap::{Parser, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "pulzr")]
#[command(about = "A high-performance load testing tool with TUI and WebSocket support")]
#[command(version = "0.1.0")]
pub struct Cli {
    #[arg(help = "Target URL to test (optional when using --scenario or --endpoints)")]
    pub url: Option<String>,

    #[arg(
        long,
        alias = "target",
        help = "Target URL to test (alternative to positional)"
    )]
    pub target_url: Option<String>,

    #[arg(long, help = "Multiple endpoints configuration file (JSON/YAML)")]
    pub endpoints: Option<PathBuf>,

    #[arg(
        long,
        short = 'c',
        alias = "connections",
        default_value = "10",
        help = "Number of concurrent requests"
    )]
    pub concurrent: usize,

    #[arg(long, short = 'r', alias = "rate", help = "Requests per second limit")]
    pub rps: Option<u64>,

    #[arg(
        long,
        short = 'd',
        help = "Test duration in seconds (default: run until stopped)"
    )]
    pub duration: Option<u64>,

    #[arg(long, short = 'm', default_value = "get", help = "HTTP method")]
    pub method: HttpMethod,

    #[arg(long, short = 'p', help = "Request payload (JSON string or file path)")]
    pub payload: Option<String>,

    #[arg(
        long,
        short = 'b',
        alias = "body",
        help = "Request body (alias for payload)"
    )]
    pub body: Option<String>,

    #[arg(
        long,
        short = 'f',
        alias = "body-file",
        help = "File to use as request body"
    )]
    pub body_file: Option<PathBuf>,

    #[arg(long, short = 'H', help = "Custom headers (format: 'Key: Value')")]
    pub headers: Vec<String>,

    #[arg(long, help = "Custom User-Agent string")]
    pub user_agent: Option<String>,

    #[arg(long, help = "Use random User-Agent for each request")]
    pub random_ua: bool,

    #[arg(long, help = "Path to custom User-Agent list file")]
    pub ua_file: Option<PathBuf>,

    #[arg(long, short = 'o', help = "Output CSV file path")]
    pub output: Option<PathBuf>,

    #[arg(long, help = "Enable WebSocket server for real-time metrics")]
    pub websocket: bool,

    #[arg(long, default_value = "9621", help = "WebSocket server port")]
    pub websocket_port: u16,

    #[arg(
        long,
        alias = "no-tui",
        help = "Run in headless mode (disable TUI display)"
    )]
    pub headless: bool,

    #[arg(long, short = 'v', help = "Verbose output")]
    pub verbose: bool,

    #[arg(
        long,
        short = 'q',
        help = "Quiet mode - minimal output (only final summary)"
    )]
    pub quiet: bool,

    #[arg(
        long,
        default_value = "detailed",
        help = "Output format (detailed, compact, minimal)"
    )]
    pub output_format: OutputFormat,

    #[arg(long, short = 't', help = "Request timeout in seconds")]
    pub timeout: Option<u64>,

    #[arg(
        long,
        short = 'l',
        help = "Print latency statistics (enables latency distribution)"
    )]
    pub latencies: bool,

    #[arg(long, short = 'n', help = "Total number of requests to make")]
    pub requests: Option<u64>,

    #[arg(long, short = 'k', help = "Skip TLS certificate verification")]
    pub insecure: bool,

    #[arg(long, help = "Path to client TLS certificate")]
    pub cert: Option<PathBuf>,

    #[arg(long, help = "Path to client TLS certificate private key")]
    pub key: Option<PathBuf>,

    #[arg(
        long,
        short = 'P',
        help = "Enhanced print control (intro,progress,result)"
    )]
    pub print: Option<String>,

    #[arg(
        long,
        alias = "no-print",
        help = "Don't output anything (compatibility mode)"
    )]
    pub no_print: bool,

    #[arg(long, short = 'O', help = "Enhanced output format (plain-text, json)")]
    pub format: Option<OutputFormatExtended>,

    #[arg(long, help = "Enable WebUI server")]
    pub webui: bool,

    #[arg(long, default_value = "9622", help = "WebUI server port")]
    pub webui_port: u16,

    #[arg(long, help = "Auto-open WebUI in default browser")]
    pub open_browser: bool,

    #[arg(long, short = 's', help = "Load test scenario from JSON/YAML file")]
    pub scenario: Option<PathBuf>,

    #[arg(long, help = "Ramp-up duration in seconds (gradual load increase)")]
    pub ramp_up: Option<u64>,

    #[arg(
        long,
        default_value = "linear",
        help = "Ramp-up pattern (linear, exponential, step)"
    )]
    pub ramp_pattern: RampPattern,

    #[arg(
        long,
        help = "Enable debug mode with detailed request/response logging"
    )]
    pub debug: bool,

    #[arg(
        long,
        default_value = "1",
        help = "Debug verbosity level (1-3): 1=basic, 2=headers, 3=full"
    )]
    pub debug_level: u8,

    // Authentication options
    #[arg(long, help = "JWT token for authentication")]
    pub jwt_token: Option<String>,

    #[arg(long, help = "JWT secret for token validation")]
    pub jwt_secret: Option<String>,

    #[arg(long, help = "JWT issuer for validation")]
    pub jwt_issuer: Option<String>,

    #[arg(long, help = "JWT audience for validation")]
    pub jwt_audience: Option<String>,

    #[arg(long, help = "Enable automatic JWT token refresh")]
    pub jwt_auto_refresh: bool,

    #[arg(long, help = "JWT token refresh endpoint URL")]
    pub jwt_refresh_endpoint: Option<String>,

    #[arg(long, help = "API key for authentication")]
    pub api_key: Option<String>,

    #[arg(long, default_value = "x-api-key", help = "API key header name")]
    pub api_key_header: String,

    #[arg(
        long,
        default_value = "header",
        help = "API key location (header, query, bearer)"
    )]
    pub api_key_location: ApiKeyLocation,

    // Prometheus options
    #[arg(long, help = "Enable Prometheus metrics endpoint")]
    pub prometheus: bool,

    #[arg(
        long,
        default_value = "9090",
        help = "Prometheus metrics endpoint port"
    )]
    pub prometheus_port: u16,

    // Grafana options
    #[arg(long, help = "List available Grafana dashboards")]
    pub list_dashboards: bool,

    #[arg(long, help = "Show information about a specific dashboard")]
    pub dashboard_info: Option<String>,

    #[arg(long, help = "Generate import instructions for a dashboard")]
    pub dashboard_import: Option<String>,

    #[arg(long, help = "Validate a dashboard file")]
    pub dashboard_validate: Option<String>,

    #[arg(long, help = "Directory containing Grafana dashboards")]
    pub dashboards_dir: Option<String>,

    // Memory optimization options
    #[arg(long, help = "Enable memory optimization with streaming stats")]
    pub memory_optimize: bool,

    #[arg(
        long,
        help = "Memory optimization profile (default, streaming, high-throughput, low-memory)"
    )]
    pub memory_profile: Option<String>,

    #[arg(
        long,
        default_value = "10000",
        help = "Maximum number of request results to keep in memory"
    )]
    pub max_results: usize,

    #[arg(
        long,
        default_value = "3600",
        help = "Maximum age of request results in seconds"
    )]
    pub max_result_age: u64,

    #[arg(long, help = "Enable automatic memory cleanup")]
    pub auto_cleanup: bool,

    #[arg(
        long,
        default_value = "60",
        help = "Memory cleanup interval in seconds"
    )]
    pub cleanup_interval: u64,

    // HTTP/2 options
    #[arg(long, alias = "http1", help = "Disable HTTP/2 (force HTTP/1.1)")]
    pub http1_only: bool,

    #[arg(long, help = "Enable HTTP/2 protocol support")]
    pub http2: bool,

    #[arg(long, help = "Force HTTP/2 prior knowledge (skip HTTP/1.1 Upgrade)")]
    pub http2_prior_knowledge: bool,

    #[arg(long, help = "Set HTTP/2 initial connection window size")]
    pub http2_initial_connection_window_size: Option<u32>,

    #[arg(long, help = "Set HTTP/2 initial stream window size")]
    pub http2_initial_stream_window_size: Option<u32>,

    #[arg(long, help = "Set HTTP/2 max frame size")]
    pub http2_max_frame_size: Option<u32>,

    // Distributed load testing options
    #[arg(long, help = "Run as distributed coordinator")]
    pub coordinator: bool,

    #[arg(long, default_value = "9630", help = "Coordinator listening port")]
    pub coordinator_port: u16,

    #[arg(
        long,
        default_value = "100",
        help = "Maximum number of workers to accept"
    )]
    pub max_workers: usize,

    #[arg(long, help = "Run as distributed worker")]
    pub worker: bool,

    #[arg(
        long,
        help = "Coordinator host to connect to (required for worker mode)"
    )]
    pub coordinator_host: Option<String>,

    #[arg(long, help = "Worker ID (auto-generated if not specified)")]
    pub worker_id: Option<String>,

    #[arg(
        long,
        default_value = "1000",
        help = "Maximum concurrent requests for this worker"
    )]
    pub worker_max_concurrent: usize,

    #[arg(long, help = "Maximum RPS for this worker")]
    pub worker_max_rps: Option<u64>,

    #[arg(
        long,
        help = "Run distributed test client (connect to coordinator and send test)"
    )]
    pub distributed_client: bool,

    #[arg(long, help = "Show usage examples and exit")]
    pub examples: bool,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Head,
    Options,
    Patch,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum RampPattern {
    Linear,
    Exponential,
    Step,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum ApiKeyLocation {
    Header,
    Query,
    Bearer,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Detailed,
    Compact,
    Minimal,
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormatExtended {
    #[value(name = "plain-text", alias = "pt")]
    PlainText,
    #[value(name = "json", alias = "j")]
    Json,
}

impl HttpMethod {
    pub fn to_reqwest_method(&self) -> reqwest::Method {
        match self {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Head => reqwest::Method::HEAD,
            HttpMethod::Options => reqwest::Method::OPTIONS,
            HttpMethod::Patch => reqwest::Method::PATCH,
        }
    }
}

impl ApiKeyLocation {
    pub fn to_auth_location(&self) -> crate::auth::ApiKeyLocation {
        match self {
            ApiKeyLocation::Header => crate::auth::ApiKeyLocation::Header,
            ApiKeyLocation::Query => crate::auth::ApiKeyLocation::Query,
            ApiKeyLocation::Bearer => crate::auth::ApiKeyLocation::Bearer,
        }
    }
}

impl Cli {
    /// Get the target URL from either positional argument or --target-url flag
    pub fn get_url(&self) -> Option<&String> {
        self.url.as_ref().or(self.target_url.as_ref())
    }

    /// Get the request body from either --payload, --body, or --body-file
    pub fn get_body(&self) -> Option<String> {
        // Priority: --body > --payload > --body-file
        if let Some(body) = &self.body {
            Some(body.clone())
        } else if let Some(payload) = &self.payload {
            Some(payload.clone())
        } else if let Some(body_file) = &self.body_file {
            // Read from file
            std::fs::read_to_string(body_file).ok()
        } else {
            None
        }
    }

    /// Check if request count mode is enabled (compatibility mode)
    pub fn is_request_count_mode(&self) -> bool {
        self.requests.is_some()
    }

    /// Get effective quiet mode (from either --quiet or --no-print)
    pub fn is_quiet(&self) -> bool {
        self.quiet || self.no_print
    }

    /// Check if running in distributed coordinator mode
    pub fn is_coordinator_mode(&self) -> bool {
        self.coordinator
    }

    /// Check if running in distributed worker mode
    pub fn is_worker_mode(&self) -> bool {
        self.worker
    }

    /// Check if running in distributed client mode
    pub fn is_distributed_client_mode(&self) -> bool {
        self.distributed_client
    }

    /// Check if any distributed mode is enabled
    pub fn is_distributed_mode(&self) -> bool {
        self.coordinator || self.worker || self.distributed_client
    }

    /// Get coordinator host for worker mode (defaults to localhost)
    pub fn get_coordinator_host(&self) -> String {
        self.coordinator_host
            .clone()
            .unwrap_or_else(|| "localhost".to_string())
    }

    /// Validate distributed mode configuration
    pub fn validate_distributed_config(&self) -> Result<(), String> {
        // Check that only one distributed mode is enabled
        let modes_count = [self.coordinator, self.worker, self.distributed_client]
            .iter()
            .filter(|&&x| x)
            .count();

        if modes_count > 1 {
            return Err("Only one distributed mode can be enabled at a time".to_string());
        }

        // Worker mode requires coordinator host
        if self.worker && self.coordinator_host.is_none() {
            return Err("Worker mode requires --coordinator-host".to_string());
        }

        // Distributed client mode requires coordinator host
        if self.distributed_client && self.coordinator_host.is_none() {
            return Err("Distributed client mode requires --coordinator-host".to_string());
        }

        Ok(())
    }
}
