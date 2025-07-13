use anyhow::{anyhow, Result};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub iat: i64,
    pub exp: i64,
    pub iss: Option<String>,
    pub aud: Option<String>,
    #[serde(flatten)]
    pub custom: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct JwtConfig {
    pub token: Option<String>,
    pub secret: Option<String>,
    pub algorithm: Algorithm,
    pub issuer: Option<String>,
    pub audience: Option<String>,
    pub refresh_threshold_minutes: i64,
    pub auto_refresh: bool,
    pub refresh_endpoint: Option<String>,
}

impl Default for JwtConfig {
    fn default() -> Self {
        Self {
            token: None,
            secret: None,
            algorithm: Algorithm::HS256,
            issuer: None,
            audience: None,
            refresh_threshold_minutes: 5,
            auto_refresh: false,
            refresh_endpoint: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JwtManager {
    config: Arc<RwLock<JwtConfig>>,
    current_token: Arc<RwLock<Option<String>>>,
}

impl JwtManager {
    pub fn new(config: JwtConfig) -> Self {
        let current_token = config.token.clone();
        Self {
            config: Arc::new(RwLock::new(config)),
            current_token: Arc::new(RwLock::new(current_token)),
        }
    }

    pub async fn get_token(&self) -> Option<String> {
        let token = self.current_token.read().await;
        token.clone()
    }

    pub async fn set_token(&self, token: String) {
        let mut current_token = self.current_token.write().await;
        *current_token = Some(token);
    }

    pub async fn validate_token(&self, token: &str) -> Result<JwtClaims> {
        let config = self.config.read().await;

        let secret = config
            .secret
            .as_ref()
            .ok_or_else(|| anyhow!("JWT secret not configured"))?;

        let decoding_key = DecodingKey::from_secret(secret.as_bytes());
        let mut validation = Validation::new(config.algorithm);

        if let Some(iss) = &config.issuer {
            validation.set_issuer(&[iss]);
        }

        if let Some(aud) = &config.audience {
            validation.set_audience(&[aud]);
        }

        let token_data = decode::<JwtClaims>(token, &decoding_key, &validation)
            .map_err(|e| anyhow!("JWT validation failed: {}", e))?;

        Ok(token_data.claims)
    }

    pub async fn is_token_expired(&self, token: &str) -> bool {
        match self.validate_token(token).await {
            Ok(claims) => {
                let now = Utc::now().timestamp();
                claims.exp <= now
            }
            Err(_) => true,
        }
    }

    pub async fn should_refresh_token(&self, token: &str) -> bool {
        let config = self.config.read().await;

        if !config.auto_refresh {
            return false;
        }

        match self.validate_token(token).await {
            Ok(claims) => {
                let now = Utc::now().timestamp();
                let refresh_threshold = now + (config.refresh_threshold_minutes * 60);
                claims.exp <= refresh_threshold
            }
            Err(_) => true,
        }
    }

    pub async fn refresh_token(&self) -> Result<String> {
        let config = self.config.read().await;

        let refresh_endpoint = config
            .refresh_endpoint
            .as_ref()
            .ok_or_else(|| anyhow!("Refresh endpoint not configured"))?;

        let current_token = self.current_token.read().await;
        let token = current_token
            .as_ref()
            .ok_or_else(|| anyhow!("No current token to refresh"))?;

        // Make a request to the refresh endpoint
        let client = reqwest::Client::new();
        let response = client
            .post(refresh_endpoint)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|e| anyhow!("Token refresh request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow!("Token refresh failed: {}", response.status()));
        }

        #[derive(Deserialize)]
        struct RefreshResponse {
            access_token: String,
        }

        let refresh_response: RefreshResponse = response
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse refresh response: {}", e))?;

        Ok(refresh_response.access_token)
    }

    pub async fn ensure_valid_token(&self) -> Result<String> {
        let current_token = self.current_token.read().await;

        if let Some(token) = current_token.as_ref() {
            // Check if token needs refresh
            if self.should_refresh_token(token).await {
                drop(current_token);
                println!("🔄 Refreshing JWT token...");

                match self.refresh_token().await {
                    Ok(new_token) => {
                        self.set_token(new_token.clone()).await;
                        println!("✅ JWT token refreshed successfully");
                        return Ok(new_token);
                    }
                    Err(e) => {
                        println!("⚠️  JWT token refresh failed: {e}");
                        return Err(e);
                    }
                }
            }

            // Check if token is still valid
            if !self.is_token_expired(token).await {
                return Ok(token.clone());
            }
        }

        Err(anyhow!("No valid JWT token available"))
    }

    pub async fn create_token(&self, sub: &str, duration_minutes: i64) -> Result<String> {
        let config = self.config.read().await;

        let secret = config
            .secret
            .as_ref()
            .ok_or_else(|| anyhow!("JWT secret not configured"))?;

        let now = Utc::now();
        let exp = now + Duration::minutes(duration_minutes);

        let claims = JwtClaims {
            sub: sub.to_string(),
            iat: now.timestamp(),
            exp: exp.timestamp(),
            iss: config.issuer.clone(),
            aud: config.audience.clone(),
            custom: HashMap::new(),
        };

        let header = Header::new(config.algorithm);
        let encoding_key = EncodingKey::from_secret(secret.as_bytes());

        let token = encode(&header, &claims, &encoding_key)
            .map_err(|e| anyhow!("JWT token creation failed: {}", e))?;

        Ok(token)
    }
}

#[derive(Debug, Clone)]
pub struct ApiKeyConfig {
    pub api_key: String,
    pub header_name: String,
    pub location: ApiKeyLocation,
}

#[derive(Debug, Clone)]
pub enum ApiKeyLocation {
    Header,
    Query,
    Bearer,
}

impl Default for ApiKeyLocation {
    fn default() -> Self {
        Self::Header
    }
}

#[derive(Debug, Clone)]
pub struct ApiKeyManager {
    config: ApiKeyConfig,
}

impl ApiKeyManager {
    pub fn new(config: ApiKeyConfig) -> Self {
        Self { config }
    }

    pub fn get_header_name(&self) -> &str {
        &self.config.header_name
    }

    pub fn get_api_key(&self) -> &str {
        &self.config.api_key
    }

    pub fn get_location(&self) -> &ApiKeyLocation {
        &self.config.location
    }

    pub fn format_auth_header(&self) -> (String, String) {
        match self.config.location {
            ApiKeyLocation::Header => {
                (self.config.header_name.clone(), self.config.api_key.clone())
            }
            ApiKeyLocation::Bearer => (
                "Authorization".to_string(),
                format!("Bearer {}", self.config.api_key),
            ),
            ApiKeyLocation::Query => {
                // This will be handled differently in the client
                (self.config.header_name.clone(), self.config.api_key.clone())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum AuthMethod {
    None,
    Jwt(JwtManager),
    ApiKey(ApiKeyManager),
}

impl Default for AuthMethod {
    fn default() -> Self {
        Self::None
    }
}

impl AuthMethod {
    pub async fn get_auth_header(&self) -> Option<(String, String)> {
        match self {
            AuthMethod::None => None,
            AuthMethod::Jwt(manager) => match manager.ensure_valid_token().await {
                Ok(token) => Some(("Authorization".to_string(), format!("Bearer {token}"))),
                Err(_) => None,
            },
            AuthMethod::ApiKey(manager) => {
                match manager.get_location() {
                    ApiKeyLocation::Query => None, // Handled in URL
                    _ => Some(manager.format_auth_header()),
                }
            }
        }
    }

    pub async fn get_query_params(&self) -> Option<(String, String)> {
        match self {
            AuthMethod::ApiKey(manager) => match manager.get_location() {
                ApiKeyLocation::Query => Some((
                    manager.get_header_name().to_string(),
                    manager.get_api_key().to_string(),
                )),
                _ => None,
            },
            _ => None,
        }
    }
}
