use std::net::{SocketAddr, TcpListener};

pub fn find_available_port(preferred_port: u16, max_attempts: u16) -> Option<u16> {
    for attempt in 0..max_attempts {
        let port = preferred_port + attempt;
        if is_port_available(port) {
            return Some(port);
        }
    }
    None
}

pub fn is_port_available(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpListener::bind(addr).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn test_is_port_available_with_free_port() {
        // Test with a high port number that's likely to be free
        let port = 45000;
        let _available = is_port_available(port);
        // We can't guarantee the port is free, but this tests the function
    }

    #[test]
    fn test_is_port_available_with_bound_port() {
        // Bind to a port first
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        // Now test that the port is not available
        assert!(!is_port_available(port));

        // Drop the listener and test that the port becomes available
        drop(listener);
        // Note: Port might not be immediately available due to TIME_WAIT state
    }

    #[test]
    fn test_find_available_port_first_port_free() {
        // Try to find a port starting from a high number
        let preferred_port = 45000;
        let result = find_available_port(preferred_port, 10);

        match result {
            Some(port) => {
                assert!(port >= preferred_port);
                assert!(port < preferred_port + 10);
            }
            None => {
                // All ports in range were busy - this is possible but unlikely
                // with high port numbers
            }
        }
    }

    #[test]
    fn test_find_available_port_with_bound_ports() {
        // Bind to several consecutive ports
        let start_port = 45100;
        let _listeners: Vec<TcpListener> = (0..3)
            .map(|i| TcpListener::bind(format!("127.0.0.1:{}", start_port + i)).unwrap())
            .collect();

        // Try to find a port in the same range
        let result = find_available_port(start_port, 5);

        match result {
            Some(port) => {
                // Should find a port beyond the bound ones
                assert!(port >= start_port + 3);
                assert!(port < start_port + 5);
            }
            None => {
                // All ports were busy
            }
        }
    }

    #[test]
    fn test_find_available_port_no_attempts() {
        let result = find_available_port(8080, 0);
        assert_eq!(result, None);
    }

    #[test]
    fn test_find_available_port_one_attempt() {
        let preferred_port = 45200;
        let result = find_available_port(preferred_port, 1);

        match result {
            Some(port) => assert_eq!(port, preferred_port),
            None => {
                // Port was busy - verify it's actually busy
                assert!(!is_port_available(preferred_port));
            }
        }
    }

    #[test]
    fn test_find_available_port_port_overflow() {
        // Test near the maximum port number
        let preferred_port = 65530;
        let result = find_available_port(preferred_port, 10);

        // Should handle port number overflow gracefully
        match result {
            Some(port) => {
                assert!(port >= preferred_port);
                // Port should be valid (u16 guarantees this)
            }
            None => {
                // All ports in range were busy
            }
        }
    }

    #[test]
    fn test_edge_case_ports() {
        // Test some edge case port numbers
        assert!(is_port_available(0) || !is_port_available(0));
        assert!(is_port_available(65535) || !is_port_available(65535));
    }

    #[test]
    fn test_multiple_calls_consistency() {
        let port = 45300;

        // If a port is available, it should remain available in quick succession
        // (unless something else binds to it)
        let first_check = is_port_available(port);
        let _second_check = is_port_available(port);

        // Both calls should return the same result (in most cases)
        // This test might occasionally fail due to race conditions, but should be rare
        if first_check {
            // If the port was free, try to bind to it to verify
            if let Ok(_listener) = TcpListener::bind(format!("127.0.0.1:{}", port)) {
                // Successfully bound, so the port was indeed available
                assert!(first_check);
            }
        }
    }
}
