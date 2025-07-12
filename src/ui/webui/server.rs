use crate::port_utils::find_available_port;
use axum::{response::Html, routing::get, Router};
use std::net::SocketAddr;
use tower_http::{cors::CorsLayer, services::ServeDir};

pub async fn start_web_server(
    port: u16,
    websocket_port: Option<u16>,
) -> Result<u16, Box<dyn std::error::Error + Send + Sync>> {
    let actual_port = find_available_port(port, 50)
        .ok_or_else(|| format!("Could not find available port starting from {}", port))?;

    let app = Router::new()
        .route("/", get(move || serve_index(websocket_port)))
        .nest_service("/assets", ServeDir::new("assets"))
        .layer(CorsLayer::permissive());

    let addr = SocketAddr::from(([127, 0, 0, 1], actual_port));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

    // Spawn the server in the background
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("WebUI server error: {}", e);
        }
    });

    // Give the server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok(actual_port)
}

async fn serve_index(websocket_port: Option<u16>) -> Html<String> {
    let default_port = 9621;
    let ws_port = websocket_port.unwrap_or(default_port);

    let html_content = include_str!("../../../assets/index.html").replace(
        "ws://${window.location.hostname}:9621",
        &format!("ws://${{window.location.hostname}}:{}", ws_port),
    );

    Html(html_content)
}

pub fn generate_index_html() -> String {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Pulzr Load Testing Dashboard</title>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }

        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
            min-height: 100vh;
            color: #333;
        }

        .container {
            max-width: 1200px;
            margin: 0 auto;
            padding: 20px;
        }

        .header {
            background: rgba(255, 255, 255, 0.95);
            border-radius: 12px;
            padding: 24px;
            margin-bottom: 24px;
            box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);
            backdrop-filter: blur(10px);
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .header h1 {
            font-size: 2rem;
            font-weight: 700;
            background: linear-gradient(45deg, #667eea, #764ba2);
            background-clip: text;
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }

        .status {
            display: flex;
            align-items: center;
            gap: 8px;
        }

        .status-label {
            font-weight: 600;
            color: #666;
        }

        .status-value {
            background: #4ade80;
            color: white;
            padding: 6px 12px;
            border-radius: 20px;
            font-size: 0.875rem;
            font-weight: 600;
        }

        .main {
            display: flex;
            flex-direction: column;
            gap: 24px;
        }

        .config-panel, .metrics-panel, .summary-panel {
            background: rgba(255, 255, 255, 0.95);
            border-radius: 12px;
            padding: 24px;
            box-shadow: 0 8px 32px rgba(0, 0, 0, 0.1);
            backdrop-filter: blur(10px);
        }

        .config-panel h2, .metrics-panel h2, .summary-panel h2 {
            font-size: 1.5rem;
            margin-bottom: 20px;
            color: #374151;
        }

        .config-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 16px;
        }

        .config-item {
            display: flex;
            flex-direction: column;
            gap: 4px;
        }

        .config-item .label {
            font-weight: 600;
            color: #6b7280;
            font-size: 0.875rem;
        }

        .config-item .value {
            font-weight: 500;
            color: #374151;
        }

        .metrics-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
            gap: 16px;
            margin-bottom: 24px;
        }

        .metric-card {
            background: linear-gradient(135deg, #f8fafc 0%, #e2e8f0 100%);
            border-radius: 8px;
            padding: 20px;
            text-align: center;
            border: 1px solid #e2e8f0;
        }

        .metric-value {
            font-size: 2rem;
            font-weight: 700;
            color: #1e293b;
            margin-bottom: 8px;
        }

        .metric-label {
            font-size: 0.875rem;
            color: #64748b;
            font-weight: 500;
        }

        .response-times {
            margin-bottom: 24px;
        }

        .response-times h3 {
            margin-bottom: 12px;
            color: #374151;
        }

        .range-info {
            display: flex;
            gap: 24px;
            font-weight: 500;
        }

        .status-codes, .errors {
            margin-bottom: 24px;
        }

        .status-codes h3, .errors h3 {
            margin-bottom: 12px;
            color: #374151;
        }

        .status-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(120px, 1fr));
            gap: 12px;
        }

        .status-item {
            background: #f8fafc;
            border-radius: 6px;
            padding: 12px;
            text-align: center;
            border: 1px solid #e2e8f0;
        }

        .status-item.success {
            background: #dcfce7;
            border-color: #bbf7d0;
        }

        .status-item.redirect {
            background: #fef3c7;
            border-color: #fde68a;
        }

        .status-item.client-error {
            background: #fecaca;
            border-color: #fca5a5;
        }

        .status-item.server-error {
            background: #fecaca;
            border-color: #f87171;
        }

        .status-code {
            display: block;
            font-weight: 700;
            font-size: 1.125rem;
        }

        .status-count {
            display: block;
            font-size: 0.875rem;
            color: #6b7280;
        }

        .error-list {
            display: flex;
            flex-direction: column;
            gap: 8px;
        }

        .error-item {
            background: #fef2f2;
            border: 1px solid #fecaca;
            border-radius: 6px;
            padding: 12px;
            display: flex;
            justify-content: space-between;
            align-items: center;
        }

        .error-message {
            color: #dc2626;
            font-size: 0.875rem;
        }

        .error-count {
            background: #dc2626;
            color: white;
            padding: 4px 8px;
            border-radius: 12px;
            font-size: 0.75rem;
            font-weight: 600;
        }

        .summary-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
            gap: 24px;
        }

        .summary-section h3 {
            margin-bottom: 16px;
            color: #374151;
            font-size: 1.125rem;
        }

        .summary-stats {
            display: flex;
            flex-direction: column;
            gap: 12px;
        }

        .stat {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 8px 0;
            border-bottom: 1px solid #e5e7eb;
        }

        .stat:last-child {
            border-bottom: none;
        }

        .stat .label {
            font-weight: 500;
            color: #6b7280;
        }

        .stat .value {
            font-weight: 600;
            color: #374151;
        }

        .stat .value.success {
            color: #059669;
        }

        .stat .value.error {
            color: #dc2626;
        }

        .waiting {
            text-align: center;
            padding: 60px 20px;
            color: #6b7280;
            font-size: 1.125rem;
        }

        @media (max-width: 768px) {
            .container {
                padding: 12px;
            }

            .header {
                flex-direction: column;
                gap: 16px;
                text-align: center;
            }

            .header h1 {
                font-size: 1.5rem;
            }

            .metrics-grid {
                grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
            }

            .summary-grid {
                grid-template-columns: 1fr;
            }
        }
    </style>
</head>
<body>
    <div id="app"></div>
    <script type="module">
        import init, { mount_to_body } from '/pkg/pulzr.js';
        
        async function run() {
            await init();
            mount_to_body();
        }
        
        run();
    </script>
</body>
</html>"#
        .to_string()
}
