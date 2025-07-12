#![allow(clippy::uninlined_format_args)]

use anyhow::Result;
use clap::Parser;
use pulzr::{
    auth::{ApiKeyConfig, ApiKeyManager, AuthMethod, JwtConfig, JwtManager},
    cli::Cli,
    client::HttpClient,
    debug::DebugConfig,
    endpoints::MultiEndpointConfig,
    export::CsvExporter,
    get_process_memory_usage, get_system_memory_info,
    grafana::GrafanaManager,
    load_tester::LoadTester,
    prometheus::PrometheusExporter,
    prometheus_server::PrometheusServer,
    ramp_up::RampUpConfig,
    rate_limiter::RequestRateLimiter,
    scenario::Scenario,
    stats::StatsCollector,
    tui::TuiApp,
    user_agent::UserAgentManager,
    websocket::{TestConfig, WebSocketMessage, WebSocketServer},
    webui::start_web_server,
    Http2Config,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;

fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

fn setup_authentication(cli: &Cli) -> Result<AuthMethod> {
    // Check if JWT authentication is configured
    if cli.jwt_token.is_some() || cli.jwt_secret.is_some() {
        let jwt_config = JwtConfig {
            token: cli.jwt_token.clone(),
            secret: cli.jwt_secret.clone(),
            algorithm: jsonwebtoken::Algorithm::HS256,
            issuer: cli.jwt_issuer.clone(),
            audience: cli.jwt_audience.clone(),
            refresh_threshold_minutes: 5,
            auto_refresh: cli.jwt_auto_refresh,
            refresh_endpoint: cli.jwt_refresh_endpoint.clone(),
        };

        let jwt_manager = JwtManager::new(jwt_config);
        return Ok(AuthMethod::Jwt(jwt_manager));
    }

    // Check if API key authentication is configured
    if let Some(api_key) = &cli.api_key {
        let api_key_config = ApiKeyConfig {
            api_key: api_key.clone(),
            header_name: cli.api_key_header.clone(),
            location: cli.api_key_location.to_auth_location(),
        };

        let api_key_manager = ApiKeyManager::new(api_key_config);
        return Ok(AuthMethod::ApiKey(api_key_manager));
    }

    // No authentication configured
    Ok(AuthMethod::None)
}

fn create_http2_config(cli: &Cli) -> Result<Http2Config> {
    // Check for conflicting options
    if cli.http2 && cli.http1_only {
        return Err(anyhow::anyhow!(
            "Cannot specify both --http2 and --http1-only"
        ));
    }

    let mut config = if cli.http1_only {
        Http2Config::http1_only()
    } else if cli.http2 {
        if cli.http2_prior_knowledge {
            Http2Config::with_prior_knowledge()
        } else {
            Http2Config::enabled()
        }
    } else {
        // Default: HTTP/1.1 for compatibility
        Http2Config::default()
    };

    // Apply custom window sizes if provided
    if let (Some(conn_window), Some(stream_window)) = (
        cli.http2_initial_connection_window_size,
        cli.http2_initial_stream_window_size,
    ) {
        config.initial_connection_window_size = Some(conn_window);
        config.initial_stream_window_size = Some(stream_window);
    } else {
        // Apply individual settings
        if let Some(conn_window) = cli.http2_initial_connection_window_size {
            config.initial_connection_window_size = Some(conn_window);
        }
        if let Some(stream_window) = cli.http2_initial_stream_window_size {
            config.initial_stream_window_size = Some(stream_window);
        }
    }

    if let Some(frame_size) = cli.http2_max_frame_size {
        config.max_frame_size = Some(frame_size);
    }

    // Validate the configuration
    config.validate()?;

    Ok(config)
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle memory optimization demo command
    if cli.memory_optimize && cli.url.is_none() && cli.scenario.is_none() && cli.endpoints.is_none()
    {
        println!("🔧 Memory Optimization Demo Mode");
        println!("This would demonstrate memory optimization features.");
        println!(
            "Current memory profile: {}",
            cli.memory_profile.as_deref().unwrap_or("default")
        );
        println!("Max results in memory: {}", cli.max_results);
        println!("Max result age: {}s", cli.max_result_age);
        println!("Auto cleanup: {}", cli.auto_cleanup);
        println!("Cleanup interval: {}s", cli.cleanup_interval);

        // Show current system memory
        match get_system_memory_info() {
            Ok(info) => info.print_summary(),
            Err(e) => println!("Could not get system memory info: {}", e),
        }

        if let Ok(process_mem) = get_process_memory_usage() {
            println!("Current process memory: {:.2} MB", process_mem);
        }

        println!("\n💡 Tips for memory optimization:");
        println!("   --memory-profile streaming     # For continuous long-running tests");
        println!("   --memory-profile low-memory    # For memory-constrained environments");
        println!("   --memory-profile high-throughput # For high-performance testing");
        println!("   --max-results 1000            # Limit memory usage");
        println!("   --auto-cleanup                # Enable periodic cleanup");

        return Ok(());
    }

    // Handle Grafana dashboard commands
    if cli.list_dashboards
        || cli.dashboard_info.is_some()
        || cli.dashboard_import.is_some()
        || cli.dashboard_validate.is_some()
    {
        let grafana_manager = GrafanaManager::new(cli.dashboards_dir.clone());

        if cli.list_dashboards {
            println!("📊 Available Grafana Dashboards:");
            match grafana_manager.list_dashboards() {
                Ok(dashboards) => {
                    if dashboards.is_empty() {
                        println!(
                            "   No dashboards found in directory: {}",
                            cli.dashboards_dir.as_deref().unwrap_or("dashboards")
                        );
                    } else {
                        for dashboard in dashboards {
                            if let Ok(info) = grafana_manager.get_dashboard_info(&dashboard) {
                                info.print_summary();
                                println!();
                            }
                        }
                    }
                }
                Err(e) => eprintln!("Error listing dashboards: {}", e),
            }
            return Ok(());
        }

        if let Some(dashboard_name) = &cli.dashboard_info {
            match grafana_manager.get_dashboard_info(dashboard_name) {
                Ok(info) => {
                    info.print_summary();
                }
                Err(e) => eprintln!("Error getting dashboard info: {}", e),
            }
            return Ok(());
        }

        if let Some(dashboard_name) = &cli.dashboard_import {
            match grafana_manager.generate_import_instructions(dashboard_name) {
                Ok(instructions) => {
                    println!("{}", instructions);
                }
                Err(e) => eprintln!("Error generating import instructions: {}", e),
            }
            return Ok(());
        }

        if let Some(dashboard_name) = &cli.dashboard_validate {
            match grafana_manager.validate_dashboard(dashboard_name) {
                Ok(issues) => {
                    if issues.is_empty() {
                        println!("✅ Dashboard '{}' is valid", dashboard_name);
                    } else {
                        println!("❌ Dashboard '{}' has issues:", dashboard_name);
                        for issue in issues {
                            println!("   - {}", issue);
                        }
                    }
                }
                Err(e) => eprintln!("Error validating dashboard: {}", e),
            }
            return Ok(());
        }
    }

    // Load scenario file if provided
    let scenario = if let Some(scenario_path) = &cli.scenario {
        Some(Scenario::load_from_file(scenario_path)?)
    } else {
        None
    };

    // Load endpoints file if provided
    let endpoints = if let Some(endpoints_path) = &cli.endpoints {
        let config = MultiEndpointConfig::load_from_file(endpoints_path)?;
        config.validate()?;
        Some(config)
    } else {
        None
    };

    // Validate that either URL, scenario, or endpoints is provided
    if cli.url.is_none() && scenario.is_none() && endpoints.is_none() {
        eprintln!("Error: Either --url, --scenario, or --endpoints must be provided");
        std::process::exit(1);
    }

    // Create stats collector and WebSocket server
    let (stats_collector, websocket_server, websocket_sender) = if cli.websocket || cli.webui {
        // Create a single stats collector that will be shared
        let stats_collector = Arc::new(StatsCollector::new());
        let ws_server = WebSocketServer::new(cli.websocket_port, Arc::clone(&stats_collector));
        let sender = ws_server.get_message_sender();

        // Add WebSocket sender to the same stats collector
        let stats_with_ws = Arc::new(stats_collector.clone_with_websocket_sender(sender.clone()));
        (stats_with_ws, Some(ws_server), Some(sender))
    } else {
        (Arc::new(StatsCollector::new()), None, None)
    };

    let user_agent_manager = Arc::new(UserAgentManager::new(
        cli.user_agent.clone(),
        cli.random_ua,
        cli.ua_file.as_deref(),
    )?);

    // Setup authentication
    let auth_method = Arc::new(setup_authentication(&cli)?);

    // Setup HTTP/2 configuration
    let http2_config = Arc::new(create_http2_config(&cli)?);

    let client = Arc::new(HttpClient::new(
        cli.url
            .clone()
            .unwrap_or_else(|| "http://placeholder.com".to_string()),
        cli.method.to_reqwest_method(),
        cli.headers.clone(),
        cli.payload.clone(),
        Arc::clone(&user_agent_manager),
        Arc::clone(&stats_collector),
        cli.timeout.map(Duration::from_secs),
        Arc::clone(&auth_method),
        Arc::clone(&http2_config),
    )?);

    let rate_limiter = Arc::new(RequestRateLimiter::new(cli.rps));

    let mut load_tester = LoadTester::new(
        Arc::clone(&client),
        Arc::clone(&rate_limiter),
        Arc::clone(&stats_collector),
        cli.concurrent,
        cli.duration.map(Duration::from_secs),
    );

    // Add scenario if provided
    if let Some(ref scenario) = scenario {
        load_tester = load_tester.with_scenario(Arc::new(scenario.clone()));
    }

    // Add endpoints if provided
    if let Some(ref endpoints) = endpoints {
        load_tester = load_tester.with_endpoints(Arc::new(endpoints.clone()));
    }

    // Add ramp-up if provided
    if let Some(ramp_duration) = cli.ramp_up {
        let ramp_config = RampUpConfig::new(
            Duration::from_secs(ramp_duration),
            cli.ramp_pattern.clone(),
            cli.concurrent,
        );
        load_tester = load_tester.with_ramp_up(ramp_config);
    }

    // Add debug configuration
    let debug_config = DebugConfig::new(cli.debug, cli.debug_level);
    load_tester = load_tester.with_debug(debug_config);

    let (actual_websocket_port, actual_webui_port) = if let Some(mut ws_server) = websocket_server {
        let ws_port = tokio::spawn(async move {
            match ws_server.start().await {
                Ok(port) => Some(port),
                Err(e) => {
                    eprintln!("WebSocket server error: {}", e);
                    None
                }
            }
        })
        .await
        .unwrap_or(None);

        let webui_port = if cli.webui {
            let webui_port = cli.webui_port;
            tokio::spawn(async move {
                match start_web_server(webui_port, ws_port).await {
                    Ok(port) => Some(port),
                    Err(e) => {
                        eprintln!("WebUI server error: {}", e);
                        None
                    }
                }
            })
            .await
            .unwrap_or(None)
        } else {
            None
        };

        let config = TestConfig {
            url: cli
                .url
                .clone()
                .unwrap_or_else(|| "Scenario Mode".to_string()),
            concurrent_requests: cli.concurrent,
            rps: cli.rps,
            duration_secs: cli.duration.unwrap_or(0), // 0 indicates unlimited
            method: format!("{:?}", cli.method),
            user_agent_mode: if cli.random_ua {
                format!("Random ({} agents)", user_agent_manager.get_agent_count())
            } else if cli.user_agent.is_some() {
                "Custom".to_string()
            } else {
                "Default (pulzr)".to_string()
            },
        };

        // Send test started message using the sender
        if let Some(sender) = &websocket_sender {
            let message = WebSocketMessage::TestStarted {
                timestamp: chrono::Utc::now(),
                config,
            };
            let _ = sender.send(message);
        }

        (ws_port, webui_port)
    } else {
        (None, None)
    };

    let (quit_sender, quit_receiver) = broadcast::channel(1);

    // Setup Prometheus server if enabled
    let prometheus_handle = if cli.prometheus {
        let prometheus_exporter = Arc::new(PrometheusExporter::new(Arc::clone(&stats_collector))?);
        let prometheus_server = PrometheusServer::new(cli.prometheus_port, prometheus_exporter);
        let quit_rx = quit_sender.subscribe();

        Some(tokio::spawn(async move {
            if let Err(e) = prometheus_server.start(quit_rx).await {
                eprintln!("Prometheus server error: {}", e);
            }
        }))
    } else {
        None
    };

    let tui_handle = if !cli.no_tui && !cli.webui {
        let mut tui_app =
            TuiApp::new(Arc::clone(&stats_collector)).with_quit_sender(quit_sender.clone());
        Some(tokio::spawn(async move {
            if let Err(e) = tui_app.run().await {
                eprintln!("TUI error: {}", e);
            }
        }))
    } else {
        None
    };

    let quit_sender_clone = quit_sender.clone();
    tokio::spawn(async move {
        if (signal::ctrl_c().await).is_ok() {
            let _ = quit_sender_clone.send(());
        }
    });

    println!("Starting load test...");
    if let Some(url) = &cli.url {
        println!("URL: {}", url);
    } else if let Some(scenario) = &scenario {
        println!(
            "Scenario: {} ({} steps)",
            scenario.name,
            scenario.steps.len()
        );
    } else if let Some(endpoints) = &endpoints {
        println!(
            "Endpoints: {} ({} endpoints)",
            endpoints.name,
            endpoints.endpoints.len()
        );
        for endpoint in &endpoints.endpoints {
            println!(
                "  - {}: {} {}",
                endpoint.name,
                endpoint.get_method(&endpoints.defaults),
                endpoint.url
            );
        }
    }
    println!("Method: {:?}", cli.method);
    if let Some(ramp_duration) = cli.ramp_up {
        println!(
            "Ramp-up: {} pattern over {}s to {} concurrent",
            format!("{:?}", cli.ramp_pattern).to_lowercase(),
            ramp_duration,
            cli.concurrent
        );
    } else {
        println!("Concurrent requests: {}", cli.concurrent);
    }
    if let Some(rps) = cli.rps {
        println!("RPS limit: {}", rps);
    }
    if let Some(duration) = cli.duration {
        println!("Duration: {}s", duration);
    } else {
        println!("Duration: Until stopped (Ctrl+C or 'q')");
    }
    println!(
        "User-Agent mode: {}",
        if cli.random_ua {
            format!("Random ({} agents)", user_agent_manager.get_agent_count())
        } else if cli.user_agent.is_some() {
            "Custom".to_string()
        } else {
            "Default (pulzr)".to_string()
        }
    );

    // Display HTTP/2 configuration
    let protocol_info = http2_config.get_protocol_info();
    println!("Protocol: {}", protocol_info.protocol);
    if !protocol_info.features.is_empty() {
        println!("Features: {}", protocol_info.features.join(", "));
    }

    if cli.debug {
        println!(
            "Debug mode: Level {} ({})",
            cli.debug_level,
            match cli.debug_level {
                1 => "Basic request/response info",
                2 => "Include headers",
                3 => "Full request/response details",
                _ => "Basic request/response info",
            }
        );
    }

    if let Some(port) = actual_websocket_port {
        println!("WebSocket server: ws://127.0.0.1:{}", port);
    }

    if let Some(port) = actual_webui_port {
        let webui_url = format!("http://127.0.0.1:{}", port);
        println!("🌐 WebUI available at: {}", webui_url);

        if cli.open_browser {
            if let Err(e) = open_browser(&webui_url) {
                println!("   Could not auto-open browser: {}", e);
                println!("   Please open the above URL manually in your browser");
            } else {
                println!("   Opening in your default browser...");
            }
        } else {
            println!("   Open the above URL in your browser to view the dashboard");
        }
    }

    if cli.prometheus {
        println!(
            "📊 Prometheus metrics available at: http://127.0.0.1:{}/metrics",
            cli.prometheus_port
        );
    }

    if !cli.no_tui && !cli.webui {
        println!("TUI: Press 'q' or Ctrl+C to quit early");
    } else {
        println!("Press Ctrl+C to quit early");
    }

    println!("\nTest running...\n");

    // Give user time to see the URLs before TUI starts
    if !cli.no_tui && !cli.webui && (actual_websocket_port.is_some() || actual_webui_port.is_some())
    {
        println!("Starting TUI in 3 seconds...");
        tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
    }

    if let Err(e) = load_tester.run_test(quit_receiver).await {
        eprintln!("Load test error: {}", e);
    }

    if let Some(handle) = tui_handle {
        handle.abort();
    }

    if let Some(handle) = prometheus_handle {
        handle.abort();
    }

    let final_summary = stats_collector.get_final_summary().await;

    if let Some(sender) = &websocket_sender {
        let message = WebSocketMessage::TestCompleted {
            timestamp: chrono::Utc::now(),
            summary: final_summary.clone(),
        };
        let _ = sender.send(message);
    }

    let exporter = CsvExporter::new(Arc::clone(&stats_collector));
    exporter.print_summary(&final_summary);

    if let Some(output_path) = cli.output {
        println!("\nExporting results to CSV...");

        let detailed_path = output_path.with_extension("detailed.csv");
        let summary_path = output_path.with_extension("summary.csv");

        if let Err(e) = exporter.export_detailed_results(&detailed_path).await {
            eprintln!("Failed to export detailed results: {}", e);
        } else {
            println!("Detailed results: {}", detailed_path.display());
        }

        if let Err(e) = exporter.export_summary(&summary_path).await {
            eprintln!("Failed to export summary: {}", e);
        } else {
            println!("Summary: {}", summary_path.display());
        }
    }

    println!("\nLoad test completed!");
    Ok(())
}
