use anyhow::{Context, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scenario {
    pub name: String,
    pub description: Option<String>,
    pub version: Option<String>,
    pub variables: Option<HashMap<String, String>>,
    pub defaults: Option<ScenarioDefaults>,
    pub steps: Vec<ScenarioStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioDefaults {
    pub concurrent: Option<usize>,
    pub duration: Option<u64>,
    pub rps: Option<u64>,
    pub timeout: Option<u64>,
    pub headers: Option<HashMap<String, String>>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioStep {
    pub name: String,
    pub url: String,
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub payload: Option<String>,
    pub timeout: Option<u64>,
    pub weight: Option<f64>,
    pub extract: Option<Vec<ScenarioExtract>>,
    pub validate: Option<Vec<ScenarioValidation>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioExtract {
    pub name: String,
    pub from: String,             // "header", "body", "status"
    pub selector: Option<String>, // JSON path, header name, etc.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioValidation {
    pub field: String,
    pub operator: String, // "eq", "ne", "gt", "lt", "contains", "regex"
    pub value: String,
}

impl Scenario {
    /// Load scenario from file (JSON or YAML based on extension)
    pub fn load_from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read scenario file: {}", path.display()))?;

        let scenario = match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => serde_json::from_str(&content)
                .with_context(|| "Failed to parse JSON scenario file")?,
            Some("yaml") | Some("yml") => serde_yaml::from_str(&content)
                .with_context(|| "Failed to parse YAML scenario file")?,
            _ => {
                // Try JSON first, then YAML
                serde_json::from_str(&content)
                    .or_else(|_| serde_yaml::from_str(&content))
                    .with_context(|| "Failed to parse scenario file as JSON or YAML")?
            }
        };

        Ok(scenario)
    }

    /// Get the effective defaults merged with CLI options
    pub fn get_effective_defaults(&self) -> ScenarioDefaults {
        self.defaults.clone().unwrap_or_default()
    }

    /// Get total weight of all steps (for weighted distribution)
    pub fn get_total_weight(&self) -> f64 {
        self.steps
            .iter()
            .map(|step| step.weight.unwrap_or(1.0))
            .sum()
    }

    /// Substitute variables in a string
    pub fn substitute_variables(&self, text: &str) -> String {
        let mut result = text.to_string();

        if let Some(variables) = &self.variables {
            for (key, value) in variables {
                let pattern = format!("${{{key}}}");
                result = result.replace(&pattern, value);
            }
        }

        result
    }
}

impl Default for ScenarioDefaults {
    fn default() -> Self {
        Self {
            concurrent: Some(10),
            duration: None,
            rps: None,
            timeout: None,
            headers: None,
            user_agent: None,
        }
    }
}

impl ScenarioStep {
    /// Get the HTTP method for this step
    pub fn get_method(&self) -> Method {
        match self
            .method
            .as_deref()
            .unwrap_or("GET")
            .to_uppercase()
            .as_str()
        {
            "POST" => Method::POST,
            "PUT" => Method::PUT,
            "DELETE" => Method::DELETE,
            "HEAD" => Method::HEAD,
            "OPTIONS" => Method::OPTIONS,
            "PATCH" => Method::PATCH,
            _ => Method::GET,
        }
    }

    /// Get the weight for this step (default 1.0)
    pub fn get_weight(&self) -> f64 {
        self.weight.unwrap_or(1.0)
    }

    /// Get timeout duration for this step
    pub fn get_timeout(&self) -> Option<Duration> {
        self.timeout.map(Duration::from_secs)
    }

    /// Substitute variables in URL and payload
    pub fn substitute_variables(&self, scenario: &Scenario) -> ScenarioStep {
        let mut step = self.clone();
        step.url = scenario.substitute_variables(&step.url);

        if let Some(payload) = &step.payload {
            step.payload = Some(scenario.substitute_variables(payload));
        }

        step
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_variable_substitution() {
        let mut variables = HashMap::new();
        variables.insert("host".to_string(), "api.example.com".to_string());
        variables.insert("version".to_string(), "v1".to_string());

        let scenario = Scenario {
            name: "test".to_string(),
            description: None,
            version: None,
            variables: Some(variables),
            defaults: None,
            steps: vec![],
        };

        let result = scenario.substitute_variables("https://${host}/${version}/users");
        assert_eq!(result, "https://api.example.com/v1/users");
    }

    #[test]
    fn test_step_method_parsing() {
        let step = ScenarioStep {
            name: "test".to_string(),
            url: "http://example.com".to_string(),
            method: Some("POST".to_string()),
            headers: None,
            payload: None,
            timeout: None,
            weight: None,
            extract: None,
            validate: None,
        };

        assert_eq!(step.get_method(), Method::POST);
    }
}
