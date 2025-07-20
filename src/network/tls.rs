use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TlsConfig {
    /// Skip TLS certificate verification (insecure mode)
    pub insecure: bool,
    /// Path to client TLS certificate file (PEM format)
    pub cert_path: Option<PathBuf>,
    /// Path to client TLS private key file (PEM format)
    pub key_path: Option<PathBuf>,
}

impl TlsConfig {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a TLS config with insecure mode enabled
    pub fn insecure() -> Self {
        Self {
            insecure: true,
            ..Default::default()
        }
    }

    /// Create a TLS config with client certificate authentication
    pub fn with_client_cert(cert_path: PathBuf, key_path: PathBuf) -> Self {
        Self {
            insecure: false,
            cert_path: Some(cert_path),
            key_path: Some(key_path),
        }
    }

    /// Create a TLS config with client certificate authentication and insecure mode
    pub fn with_client_cert_insecure(cert_path: PathBuf, key_path: PathBuf) -> Self {
        Self {
            insecure: true,
            cert_path: Some(cert_path),
            key_path: Some(key_path),
        }
    }

    /// Validate the TLS configuration
    pub fn validate(&self) -> Result<()> {
        // If cert_path is provided, key_path must also be provided
        if self.cert_path.is_some() && self.key_path.is_none() {
            return Err(anyhow::anyhow!(
                "Client certificate path provided but private key path is missing. Both --cert and --key must be specified together."
            ));
        }

        // If key_path is provided, cert_path must also be provided
        if self.key_path.is_some() && self.cert_path.is_none() {
            return Err(anyhow::anyhow!(
                "Private key path provided but client certificate path is missing. Both --cert and --key must be specified together."
            ));
        }

        // Validate that certificate and key files exist and are readable
        if let Some(cert_path) = &self.cert_path {
            if !cert_path.exists() {
                return Err(anyhow::anyhow!(
                    "Client certificate file not found: {}",
                    cert_path.display()
                ));
            }

            if !cert_path.is_file() {
                return Err(anyhow::anyhow!(
                    "Client certificate path is not a file: {}",
                    cert_path.display()
                ));
            }

            // Try to read the file to ensure it's accessible
            std::fs::read(cert_path).map_err(|e| {
                anyhow::anyhow!(
                    "Cannot read client certificate file {}: {}",
                    cert_path.display(),
                    e
                )
            })?;
        }

        if let Some(key_path) = &self.key_path {
            if !key_path.exists() {
                return Err(anyhow::anyhow!(
                    "Private key file not found: {}",
                    key_path.display()
                ));
            }

            if !key_path.is_file() {
                return Err(anyhow::anyhow!(
                    "Private key path is not a file: {}",
                    key_path.display()
                ));
            }

            // Try to read the file to ensure it's accessible
            std::fs::read(key_path).map_err(|e| {
                anyhow::anyhow!("Cannot read private key file {}: {}", key_path.display(), e)
            })?;
        }

        Ok(())
    }

    /// Apply TLS configuration to a reqwest ClientBuilder
    pub fn apply_to_client_builder(
        &self,
        builder: reqwest::ClientBuilder,
    ) -> Result<reqwest::ClientBuilder> {
        let mut builder = builder;

        // Apply insecure mode (skip certificate verification)
        if self.insecure {
            builder = builder
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true);
        }

        // Apply client certificate authentication if configured
        if let (Some(cert_path), Some(key_path)) = (&self.cert_path, &self.key_path) {
            // Read certificate and key files
            let cert_pem = std::fs::read(cert_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to read client certificate file {}: {}",
                    cert_path.display(),
                    e
                )
            })?;

            let key_pem = std::fs::read(key_path).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to read private key file {}: {}",
                    key_path.display(),
                    e
                )
            })?;

            // Create reqwest Identity from certificate and key
            // Combine certificate and key into a single PEM block
            let combined_pem = [&cert_pem[..], &key_pem[..]].concat();

            let identity = reqwest::Identity::from_pem(&combined_pem).map_err(|e| {
                anyhow::anyhow!(
                    "Failed to create TLS identity from certificate {} and key {}: {}",
                    cert_path.display(),
                    key_path.display(),
                    e
                )
            })?;

            builder = builder.identity(identity);
        }

        Ok(builder)
    }

    /// Check if client certificate authentication is configured
    pub fn has_client_cert(&self) -> bool {
        self.cert_path.is_some() && self.key_path.is_some()
    }

    /// Get TLS configuration summary for display
    pub fn get_summary(&self) -> TlsInfo {
        TlsInfo {
            mode: if self.insecure {
                "Insecure (skip certificate verification)".to_string()
            } else {
                "Secure (verify certificates)".to_string()
            },
            client_cert: self.has_client_cert(),
            cert_path: self.cert_path.clone(),
            key_path: self.key_path.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsInfo {
    pub mode: String,
    pub client_cert: bool,
    pub cert_path: Option<PathBuf>,
    pub key_path: Option<PathBuf>,
}

impl TlsInfo {
    pub fn print_summary(&self) {
        println!("🔒 TLS Configuration:");
        println!("   Mode: {}", self.mode);

        if self.client_cert {
            println!("   Client Certificate: Enabled");
            if let Some(cert_path) = &self.cert_path {
                println!("   Certificate: {}", cert_path.display());
            }
            if let Some(key_path) = &self.key_path {
                println!("   Private Key: {}", key_path.display());
            }
        } else {
            println!("   Client Certificate: Disabled");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_cert_files() -> Result<(TempDir, PathBuf, PathBuf)> {
        let temp_dir = TempDir::new()?;

        let cert_path = temp_dir.path().join("test.crt");
        let key_path = temp_dir.path().join("test.key");

        // Create dummy PEM files for testing
        let dummy_cert = "-----BEGIN CERTIFICATE-----\nMIIBkTCB+wIJAK7VCxPsh+XjMA0GCSqGSIb3DQEBCwUAMBQxEjAQBgNVBAMMCWxv\nY2FsaG9zdDAeFw0yMTEwMjcwNzU4NDlaFw0yMjEwMjcwNzU4NDlaMBQxEjAQBgNV\nBAMMCWxvY2FsaG9zdDCBnzANBgkqhkiG9w0BAQEFAAOBjQAwgYkCgYEAzBQ5j7ks\n9l8rMQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eF\nlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M\n9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQIDAQABMA0GCSqGSIb3DQEBCwUAA4GB\nAGq2oO/FGJ4jVhw1zOw4VKaAG4HJ3l4JO7S5xmJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4\nJGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4\nJGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4\n-----END CERTIFICATE-----\n";

        let dummy_key = "-----BEGIN PRIVATE KEY-----\nMIICdgIBADANBgkqhkiG9w0BAQEFAASCAmAwggJcAgEAAoGBAMwUOY+5LPZfKzEN\njPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcU\nKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNn\nhZUNjPcUKFNnhZUNjPcUKFNnhZUCAwEAAQJBAKq2tQoU2eFlQ2M9xQoU2eFlQ2M9\nxQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU\n2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFl\nQ2M9xQECQQDZHJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5\nVGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5\nVGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5VGJ4JGJ5\nVGJ4AkEAzBQ5j7ks9l8rMQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQo\nU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eF\nlQ2M9xQoU2eFlQ2M9xQoU2eFlQ2M9xQoU2eFlQJBAMwUOY+5LPZfKzENjPcUKFNn\nhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUN\njPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcUKFNnhZUNjPcU\nKFNnhZUCQQDMFDmPuSz2XysxDYz3FChTZ4WVDYz3FChTZ4WVDYz3FChTZ4WVDYz3\nFChTZ4WVDYz3FChTZ4WVDYz3FChTZ4WVDYz3FChTZ4WVDYz3FChTZ4WVDYz3FChT\nZ4WVDYz3FChTZ4WVDYz3FChTZ4WVDYz3FChTZ4WV\n-----END PRIVATE KEY-----\n";

        fs::write(&cert_path, dummy_cert)?;
        fs::write(&key_path, dummy_key)?;

        Ok((temp_dir, cert_path, key_path))
    }

    #[test]
    fn test_default_config() {
        let config = TlsConfig::default();
        assert!(!config.insecure);
        assert!(config.cert_path.is_none());
        assert!(config.key_path.is_none());
        assert!(!config.has_client_cert());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_insecure_config() {
        let config = TlsConfig::insecure();
        assert!(config.insecure);
        assert!(config.cert_path.is_none());
        assert!(config.key_path.is_none());
        assert!(!config.has_client_cert());
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_client_cert_config() -> Result<()> {
        let (_temp_dir, cert_path, key_path) = create_test_cert_files()?;
        let config = TlsConfig::with_client_cert(cert_path.clone(), key_path.clone());

        assert!(!config.insecure);
        assert_eq!(config.cert_path, Some(cert_path));
        assert_eq!(config.key_path, Some(key_path));
        assert!(config.has_client_cert());
        assert!(config.validate().is_ok());

        Ok(())
    }

    #[test]
    fn test_client_cert_insecure_config() -> Result<()> {
        let (_temp_dir, cert_path, key_path) = create_test_cert_files()?;
        let config = TlsConfig::with_client_cert_insecure(cert_path.clone(), key_path.clone());

        assert!(config.insecure);
        assert_eq!(config.cert_path, Some(cert_path));
        assert_eq!(config.key_path, Some(key_path));
        assert!(config.has_client_cert());
        assert!(config.validate().is_ok());

        Ok(())
    }

    #[test]
    fn test_validation_cert_without_key() {
        let config = TlsConfig {
            insecure: false,
            cert_path: Some(PathBuf::from("/nonexistent/cert.pem")),
            key_path: None,
        };

        assert!(config.validate().is_err());
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("private key path is missing"));
    }

    #[test]
    fn test_validation_key_without_cert() {
        let config = TlsConfig {
            insecure: false,
            cert_path: None,
            key_path: Some(PathBuf::from("/nonexistent/key.pem")),
        };

        assert!(config.validate().is_err());
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("certificate path is missing"));
    }

    #[test]
    fn test_validation_nonexistent_cert() {
        let config = TlsConfig {
            insecure: false,
            cert_path: Some(PathBuf::from("/nonexistent/cert.pem")),
            key_path: Some(PathBuf::from("/nonexistent/key.pem")),
        };

        assert!(config.validate().is_err());
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("certificate file not found"));
    }

    #[test]
    fn test_validation_nonexistent_key() -> Result<()> {
        let temp_dir = TempDir::new()?;
        let cert_path = temp_dir.path().join("cert.pem");
        fs::write(&cert_path, "dummy cert")?;

        let config = TlsConfig {
            insecure: false,
            cert_path: Some(cert_path),
            key_path: Some(PathBuf::from("/nonexistent/key.pem")),
        };

        assert!(config.validate().is_err());
        let error = config.validate().unwrap_err();
        assert!(error.to_string().contains("key file not found"));

        Ok(())
    }

    #[test]
    fn test_get_summary() -> Result<()> {
        let (_temp_dir, cert_path, key_path) = create_test_cert_files()?;

        // Test default config
        let config = TlsConfig::default();
        let info = config.get_summary();
        assert!(info.mode.contains("Secure"));
        assert!(!info.client_cert);

        // Test insecure config
        let config = TlsConfig::insecure();
        let info = config.get_summary();
        assert!(info.mode.contains("Insecure"));
        assert!(!info.client_cert);

        // Test client cert config
        let config = TlsConfig::with_client_cert(cert_path.clone(), key_path.clone());
        let info = config.get_summary();
        assert!(info.mode.contains("Secure"));
        assert!(info.client_cert);
        assert_eq!(info.cert_path, Some(cert_path));
        assert_eq!(info.key_path, Some(key_path));

        Ok(())
    }

    #[test]
    fn test_apply_to_client_builder_insecure() {
        let config = TlsConfig::insecure();
        let builder = reqwest::Client::builder();

        // This should not panic and should return a builder
        let result = config.apply_to_client_builder(builder);
        assert!(result.is_ok());
    }

    #[test]
    fn test_apply_to_client_builder_with_cert() -> Result<()> {
        let (_temp_dir, cert_path, key_path) = create_test_cert_files()?;
        let config = TlsConfig::with_client_cert(cert_path, key_path);
        let builder = reqwest::Client::builder();

        // This should not panic but may fail due to invalid dummy cert format
        let result = config.apply_to_client_builder(builder);

        // We expect this to fail due to dummy cert format, but the function should handle the error gracefully
        match result {
            Ok(_) => {
                // If it succeeds, that's also fine (reqwest might be more lenient)
            }
            Err(e) => {
                // We expect this to fail with a TLS identity error due to dummy cert
                assert!(
                    e.to_string().contains("TLS identity") || e.to_string().contains("identity")
                );
            }
        }

        Ok(())
    }
}
