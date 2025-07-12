use clap::Parser;
use pulzr::cli::Cli;

#[test]
fn test_cli_basic_parsing() {
    let args = vec!["pulzr", "--url", "http://example.com"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.url, Some("http://example.com".to_string()));
    assert_eq!(cli.concurrent, 10); // Default value
    assert_eq!(cli.duration, None);
}

#[test]
fn test_cli_with_options() {
    let args = vec![
        "pulzr",
        "--url",
        "http://example.com",
        "-c",
        "50",
        "-d",
        "30",
        "--rps",
        "100",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert_eq!(cli.url, Some("http://example.com".to_string()));
    assert_eq!(cli.concurrent, 50);
    assert_eq!(cli.duration, Some(30));
    assert_eq!(cli.rps, Some(100));
}

#[test]
fn test_cli_user_agent_options() {
    let args = vec![
        "pulzr",
        "--url",
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
    let args = vec!["pulzr", "--url", "http://example.com", "--random-ua"];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.random_ua);
    assert_eq!(cli.user_agent, None);
}

#[test]
fn test_cli_webui_options() {
    let args = vec![
        "pulzr",
        "--url",
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
        "--url",
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
    assert_eq!(cli.url, None);
}

#[test]
fn test_cli_headers() {
    let args = vec![
        "pulzr",
        "--url",
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
        "--url",
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
        "--url",
        "http://example.com",
        "--debug",
        "--debug-level",
        "3",
    ];
    let cli = Cli::try_parse_from(args).unwrap();

    assert!(cli.debug);
    assert_eq!(cli.debug_level, 3);
}
