use std::net::TcpListener;

/// Test utilities for integration tests
pub fn get_free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// Mock HTTP server for testing
pub struct MockHttpServer {
    pub port: u16,
    pub base_url: String,
}

impl MockHttpServer {
    pub fn new() -> Self {
        let port = get_free_port();
        let base_url = format!("http://127.0.0.1:{port}");
        Self { port, base_url }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }
}
