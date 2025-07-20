use clap::Parser;
use pulzr::cli::{Cli, OutputFormatExtended};
use pulzr::TlsConfig;

#[test]
fn test_cli_basic_parsing() {
    let args = vec!["pulzr", "http://example.com"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.get_url(), Some(&"http://example.com".to_string()));
    assert_eq!(cli.concurrent, 10); // Default value
    assert_eq!(cli.duration, None);
}

#[test]
fn test_cli_with_options() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "-c",
        "50",
        "-d",
        "30",
        "--rps",
        "100",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.get_url(), Some(&"http://example.com".to_string()));
    assert_eq!(cli.concurrent, 50);
    assert_eq!(cli.duration, Some(30));
    assert_eq!(cli.rps, Some(100));
}

#[test]
fn test_cli_user_agent_options() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "--user-agent",
        "CustomAgent/1.0",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.user_agent, Some("CustomAgent/1.0".to_string()));
    assert!(!cli.random_ua);
}

#[test]
fn test_cli_random_user_agent() {
    let args = vec!["pulzr", "http://example.com", "--random-ua"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.random_ua);
    assert_eq!(cli.user_agent, None);
}

#[test]
fn test_cli_webui_options() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "--webui",
        "--webui-port",
        "8080",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.webui);
    assert_eq!(cli.webui_port, 8080);
}

#[test]
fn test_cli_output_options() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "--output",
        "/tmp/results.csv",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.output, Some("/tmp/results.csv".into()));
}

#[test]
fn test_cli_no_url_is_ok() {
    // URL is optional when using endpoints or scenario
    let args = vec!["pulzr"];
    let result = Cli::try_parse_from(args);

    assert!(result.is_ok());
    let cli = result.unwrap();
    assert_eq!(cli.get_url(), None);
}

#[test]
fn test_cli_headers() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "-H",
        "Content-Type: application/json",
        "-H",
        "Authorization: Bearer token",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.headers.len(), 2);
    assert!(cli
        .headers
        .contains(&"Content-Type: application/json".to_string()));
    assert!(cli
        .headers
        .contains(&"Authorization: Bearer token".to_string()));
}

#[test]
fn test_cli_websocket_options() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "--websocket",
        "--websocket-port",
        "9999",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.websocket);
    assert_eq!(cli.websocket_port, 9999);
}

#[test]
fn test_cli_debug_options() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "--debug",
        "--debug-level",
        "3",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.debug);
    assert_eq!(cli.debug_level, 3);
}

// Compatibility tests
#[test]
fn test_cli_positional_url() {
    let args = vec!["pulzr", "http://example.com"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.get_url(), Some(&"http://example.com".to_string()));
}

#[test]
fn test_cli_enhanced_aliases() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "--connections",
        "50",
        "--rate",
        "100",
        "-t",
        "30",
        "-l",
        "-k",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.get_url(), Some(&"http://example.com".to_string()));
    assert_eq!(cli.concurrent, 50);
    assert_eq!(cli.rps, Some(100));
    assert_eq!(cli.timeout, Some(30));
    assert!(cli.latencies);
    assert!(cli.insecure);
}

#[test]
fn test_cli_body_options() {
    let args = vec!["pulzr", "http://example.com", "--body", "test body content"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.get_body(), Some("test body content".to_string()));
}

#[test]
fn test_cli_enhanced_format() {
    let args = vec!["pulzr", "http://example.com", "--format", "json"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(matches!(cli.format, Some(OutputFormatExtended::Json)));
}

#[test]
fn test_cli_enhanced_format_plain_text() {
    let args = vec!["pulzr", "http://example.com", "--format", "plain-text"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(matches!(cli.format, Some(OutputFormatExtended::PlainText)));
}

#[test]
fn test_cli_enhanced_format_aliases() {
    let args_json = vec!["pulzr", "http://example.com", "--format", "j"];
    let cli = Cli::try_parse_from(args_json).unwrap();
    assert!(matches!(cli.format, Some(OutputFormatExtended::Json)));

    let args_pt = vec!["pulzr", "http://example.com", "--format", "pt"];
    let cli = Cli::try_parse_from(args_pt).unwrap();
    assert!(matches!(cli.format, Some(OutputFormatExtended::PlainText)));
}

#[test]
fn test_cli_print_control() {
    let args = vec!["pulzr", "http://example.com", "--print", "intro,result"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.print, Some("intro,result".to_string()));
}

#[test]
fn test_cli_no_print() {
    let args = vec!["pulzr", "http://example.com", "--no-print"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.no_print);
    assert!(cli.is_quiet()); // Should be considered quiet
}

#[test]
fn test_cli_body_priority() {
    // Test that --body takes priority over --payload
    let args = vec![
        "pulzr",
        "http://example.com",
        "--payload",
        "payload content",
        "--body",
        "body content",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.get_body(), Some("body content".to_string()));
}

#[test]
fn test_cli_quiet_modes() {
    // Test native --quiet
    let args_quiet = vec!["pulzr", "http://example.com", "--quiet"];
    let cli = Cli::try_parse_from(args_quiet).unwrap();
    assert!(cli.is_quiet());

    // Test enhanced --no-print
    let args_no_print = vec!["pulzr", "http://example.com", "--no-print"];
    let cli = Cli::try_parse_from(args_no_print).unwrap();
    assert!(cli.is_quiet());
}

#[test]
fn test_cli_url_precedence() {
    // Test that positional URL takes precedence
    let args = vec![
        "pulzr",
        "http://positional.com",
        "--target-url",
        "http://flag.com",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.get_url(), Some(&"http://positional.com".to_string()));
}

#[test]
fn test_cli_request_count_mode() {
    let args = vec!["pulzr", "http://example.com", "-n", "1000"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.requests, Some(1000));
    assert!(cli.is_request_count_mode());
}

#[test]
fn test_cli_http_protocols() {
    let args = vec!["pulzr", "http://example.com", "--http1"];
    let cli = Cli::try_parse_from(args).unwrap();
    assert!(cli.http1_only);

    let args = vec!["pulzr", "http://example.com", "--http2"];
    let cli = Cli::try_parse_from(args).unwrap();
    assert!(cli.http2);
}

#[test]
fn test_cli_tls_options() {
    let args = vec![
        "pulzr",
        "http://example.com",
        "--cert",
        "/path/to/cert.pem",
        "--key",
        "/path/to/key.pem",
        "--insecure",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.cert, Some("/path/to/cert.pem".into()));
    assert_eq!(cli.key, Some("/path/to/key.pem".into()));
    assert!(cli.insecure);
}

#[test]
fn test_cli_request_count_vs_duration() {
    // Test that request count mode takes precedence over duration
    let args = vec!["pulzr", "http://example.com", "-n", "500", "-d", "30"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.requests, Some(500));
    assert_eq!(cli.duration, Some(30));
    assert!(cli.is_request_count_mode());
}

#[test]
fn test_cli_examples_flag() {
    let args = vec!["pulzr", "--examples"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.examples);
}

// TLS Configuration Tests
#[test]
fn test_tls_config_creation_from_cli() {
    // Test default TLS config
    let args = vec!["pulzr", "http://example.com"];
    let _cli = Cli::try_parse_from(args).unwrap();

    let tls_config = TlsConfig::new();
    assert!(!tls_config.insecure);
    assert!(tls_config.cert_path.is_none());
    assert!(tls_config.key_path.is_none());
    assert!(tls_config.validate().is_ok());
}

#[test]
fn test_tls_config_insecure_mode() {
    let args = vec!["pulzr", "http://example.com", "--insecure"];
    let cli = Cli::try_parse_from(args).unwrap();

    let mut tls_config = TlsConfig::new();
    if cli.insecure {
        tls_config.insecure = true;
    }

    assert!(tls_config.insecure);
    assert!(tls_config.validate().is_ok());
}

#[test]
fn test_tls_config_validation_missing_cert() {
    use std::path::PathBuf;

    let mut tls_config = TlsConfig::new();
    tls_config.cert_path = Some(PathBuf::from("/nonexistent/cert.pem"));
    // Missing key_path should cause validation error

    assert!(tls_config.validate().is_err());
    let error = tls_config.validate().unwrap_err();
    assert!(error.to_string().contains("private key path is missing"));
}

#[test]
fn test_tls_config_validation_missing_key() {
    use std::path::PathBuf;

    let mut tls_config = TlsConfig::new();
    tls_config.key_path = Some(PathBuf::from("/nonexistent/key.pem"));
    // Missing cert_path should cause validation error

    assert!(tls_config.validate().is_err());
    let error = tls_config.validate().unwrap_err();
    assert!(error.to_string().contains("certificate path is missing"));
}

#[test]
fn test_tls_config_apply_insecure_mode() {
    let tls_config = TlsConfig::insecure();
    let builder = reqwest::Client::builder();

    // This should not panic
    let result = tls_config.apply_to_client_builder(builder);
    assert!(result.is_ok());
}

#[test]
fn test_tls_info_summary() {
    // Test default config summary
    let tls_config = TlsConfig::new();
    let info = tls_config.get_summary();
    assert!(info.mode.contains("Secure"));
    assert!(!info.client_cert);

    // Test insecure config summary
    let tls_config = TlsConfig::insecure();
    let info = tls_config.get_summary();
    assert!(info.mode.contains("Insecure"));
    assert!(!info.client_cert);
}

#[test]
fn test_tls_config_with_cli_args() {
    use std::path::PathBuf;

    // Test creating TLS config from CLI args with cert and key
    let args = vec![
        "pulzr",
        "http://example.com",
        "--cert",
        "/path/to/cert.pem",
        "--key",
        "/path/to/key.pem",
        "--insecure",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    // Simulate the logic from main.rs
    let mut tls_config = TlsConfig::new();

    if cli.insecure {
        tls_config.insecure = true;
    }

    if let (Some(cert), Some(key)) = (&cli.cert, &cli.key) {
        tls_config.cert_path = Some(cert.clone());
        tls_config.key_path = Some(key.clone());
    }

    assert!(tls_config.insecure);
    assert_eq!(
        tls_config.cert_path,
        Some(PathBuf::from("/path/to/cert.pem"))
    );
    assert_eq!(tls_config.key_path, Some(PathBuf::from("/path/to/key.pem")));
    assert!(tls_config.has_client_cert());

    // Note: validation will fail since files don't exist, but structure is correct
    assert!(tls_config.validate().is_err());
}
