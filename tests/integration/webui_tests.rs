use crate::common::{get_free_port, MockHttpServer};

#[tokio::test]
async fn test_mock_server_creation() {
    let server = MockHttpServer::new();
    assert!(server.port > 0);
    assert!(server.base_url.starts_with("http://127.0.0.1:"));
}

#[tokio::test]
async fn test_url_building() {
    let server = MockHttpServer::new();
    let url = server.url("/test");
    assert!(url.contains("/test"));
    assert!(url.starts_with("http://127.0.0.1:"));
}

#[tokio::test]
async fn test_httpbin_connectivity() {
    // Test that we can reach httpbin for integration tests
    let client = reqwest::Client::new();
    let response = client.get("https://httpbin.org/status/200").send().await;

    match response {
        Ok(resp) => {
            assert_eq!(resp.status(), 200);
        }
        Err(_) => {
            // Skip test if httpbin is not available
            println!("httpbin.org not available, skipping connectivity test");
        }
    }
}

#[tokio::test]
async fn test_port_allocation() {
    let port = get_free_port();
    assert!(port > 0);
    // Port is valid (u16 guarantees this)
}

// Note: More comprehensive WebUI tests would require running the actual server
// These are basic structural tests to ensure the test infrastructure works
