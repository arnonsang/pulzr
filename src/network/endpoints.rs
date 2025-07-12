use anyhow::{Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiEndpointConfig {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub defaults: Option<EndpointDefaults>,
    pub endpoints: Vec<Endpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDefaults {
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub timeout: Option<u64>,
    pub weight: Option<f64>,
    pub rps_share: Option<f64>, // Percentage of total RPS for this endpoint
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Endpoint {
    pub name: String,
    pub url: String,
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub payload: Option<String>,
    pub timeout: Option<u64>,
    pub weight: Option<f64>,
    pub rps_share: Option<f64>,
    pub expected_status: Option<Vec<u16>>, // Expected status codes for this endpoint
}

impl MultiEndpointConfig {
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read endpoint config file: {:?}", path.as_ref()))?;

        // Try JSON first, then YAML
        if let Ok(config) = serde_json::from_str::<MultiEndpointConfig>(&content) {
            Ok(config)
        } else {
            serde_yaml::from_str::<MultiEndpointConfig>(&content).with_context(|| {
                format!(
                    "Failed to parse endpoint config file as JSON or YAML: {:?}",
                    path.as_ref()
                )
            })
        }
    }

    /// Get total weight of all endpoints
    pub fn total_weight(&self) -> f64 {
        self.endpoints
            .iter()
            .map(|endpoint| endpoint.get_weight(&self.defaults))
            .sum()
    }

    /// Validate the endpoint configuration
    pub fn validate(&self) -> Result<()> {
        if self.endpoints.is_empty() {
            anyhow::bail!("At least one endpoint must be specified");
        }

        // Check for unique endpoint names
        let mut names = std::collections::HashSet::new();
        for endpoint in &self.endpoints {
            if !names.insert(&endpoint.name) {
                anyhow::bail!("Duplicate endpoint name: {}", endpoint.name);
            }
        }

        // Validate RPS shares if specified
        let total_rps_share: f64 = self
            .endpoints
            .iter()
            .filter_map(|e| {
                e.rps_share
                    .or_else(|| self.defaults.as_ref().and_then(|d| d.rps_share))
            })
            .sum();

        if total_rps_share > 0.0 && (total_rps_share - 1.0).abs() > 0.01 {
            anyhow::bail!(
                "RPS shares must sum to 1.0 (100%), got: {:.2}",
                total_rps_share
            );
        }

        Ok(())
    }
}

impl Endpoint {
    pub fn get_method(&self, defaults: &Option<EndpointDefaults>) -> Method {
        let method_str = self
            .method
            .as_ref()
            .or_else(|| defaults.as_ref().and_then(|d| d.method.as_ref()))
            .map(|s| s.as_str())
            .unwrap_or("GET");

        match method_str.to_uppercase().as_str() {
            "GET" => Method::GET,
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            "PATCH" => Method::PATCH,
            _ => Method::GET,
        }
    }

    pub fn get_weight(&self, defaults: &Option<EndpointDefaults>) -> f64 {
        self.weight
            .or_else(|| defaults.as_ref().and_then(|d| d.weight))
            .unwrap_or(1.0)
    }

    pub fn get_timeout(&self, defaults: &Option<EndpointDefaults>) -> Option<std::time::Duration> {
        self.timeout
            .or_else(|| defaults.as_ref().and_then(|d| d.timeout))
            .map(std::time::Duration::from_secs)
    }

    pub fn get_rps_share(&self, defaults: &Option<EndpointDefaults>) -> Option<f64> {
        self.rps_share
            .or_else(|| defaults.as_ref().and_then(|d| d.rps_share))
    }

    pub fn get_headers(&self, defaults: &Option<EndpointDefaults>) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        // Add default headers first
        if let Some(defaults) = defaults {
            if let Some(default_headers) = &defaults.headers {
                headers.extend(default_headers.clone());
            }
        }

        // Override with endpoint-specific headers
        if let Some(endpoint_headers) = &self.headers {
            headers.extend(endpoint_headers.clone());
        }

        headers
    }

    pub fn is_expected_status(&self, status: u16) -> bool {
        if let Some(expected) = &self.expected_status {
            expected.contains(&status)
        } else {
            // Default to 2xx status codes
            (200..300).contains(&status)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_endpoint_weight() {
        let endpoint = Endpoint {
            name: "test".to_string(),
            url: "http://example.com".to_string(),
            method: None,
            headers: None,
            payload: None,
            timeout: None,
            weight: Some(2.5),
            rps_share: None,
            expected_status: None,
        };

        let defaults = Some(EndpointDefaults {
            method: None,
            headers: None,
            timeout: None,
            weight: Some(1.0),
            rps_share: None,
        });

        assert_eq!(endpoint.get_weight(&defaults), 2.5);

        let endpoint_no_weight = Endpoint {
            name: "test".to_string(),
            url: "http://example.com".to_string(),
            method: None,
            headers: None,
            payload: None,
            timeout: None,
            weight: None,
            rps_share: None,
            expected_status: None,
        };

        assert_eq!(endpoint_no_weight.get_weight(&defaults), 1.0);
        assert_eq!(endpoint_no_weight.get_weight(&None), 1.0);
    }

    #[test]
    fn test_expected_status() {
        let endpoint = Endpoint {
            name: "test".to_string(),
            url: "http://example.com".to_string(),
            method: None,
            headers: None,
            payload: None,
            timeout: None,
            weight: None,
            rps_share: None,
            expected_status: Some(vec![200, 201, 404]),
        };

        assert!(endpoint.is_expected_status(200));
        assert!(endpoint.is_expected_status(201));
        assert!(endpoint.is_expected_status(404));
        assert!(!endpoint.is_expected_status(500));

        let endpoint_default = Endpoint {
            name: "test".to_string(),
            url: "http://example.com".to_string(),
            method: None,
            headers: None,
            payload: None,
            timeout: None,
            weight: None,
            rps_share: None,
            expected_status: None,
        };

        assert!(endpoint_default.is_expected_status(200));
        assert!(endpoint_default.is_expected_status(201));
        assert!(!endpoint_default.is_expected_status(404));
        assert!(!endpoint_default.is_expected_status(500));
    }
}
