use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrafanaDashboard {
    pub title: String,
    pub uid: String,
    pub version: i32,
    pub description: String,
    pub tags: Vec<String>,
    pub refresh: String,
    pub time: TimeRange,
    pub panels: Vec<serde_json::Value>,
    pub templating: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrafanaDataSource {
    pub name: String,
    pub r#type: String,
    pub url: String,
    pub access: String,
    pub is_default: bool,
}

pub struct GrafanaManager {
    dashboards_dir: String,
}

impl GrafanaManager {
    pub fn new(dashboards_dir: Option<String>) -> Self {
        Self {
            dashboards_dir: dashboards_dir.unwrap_or_else(|| "dashboards".to_string()),
        }
    }

    /// Load a dashboard from file
    pub fn load_dashboard(&self, dashboard_name: &str) -> Result<GrafanaDashboard> {
        let dashboard_path = Path::new(&self.dashboards_dir).join(format!("{dashboard_name}.json"));

        if !dashboard_path.exists() {
            return Err(anyhow::anyhow!(
                "Dashboard file not found: {}",
                dashboard_path.display()
            ));
        }

        let content = fs::read_to_string(&dashboard_path)
            .map_err(|e| anyhow::anyhow!("Failed to read dashboard file: {}", e))?;

        let dashboard: GrafanaDashboard = serde_json::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Failed to parse dashboard JSON: {}", e))?;

        Ok(dashboard)
    }

    /// List available dashboards
    pub fn list_dashboards(&self) -> Result<Vec<String>> {
        let dashboards_path = Path::new(&self.dashboards_dir);

        if !dashboards_path.exists() {
            return Ok(Vec::new());
        }

        let mut dashboards = Vec::new();

        for entry in fs::read_dir(dashboards_path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    dashboards.push(name.to_string());
                }
            }
        }

        dashboards.sort();
        Ok(dashboards)
    }

    /// Get dashboard information without loading full content
    pub fn get_dashboard_info(&self, dashboard_name: &str) -> Result<DashboardInfo> {
        let dashboard = self.load_dashboard(dashboard_name)?;

        Ok(DashboardInfo {
            name: dashboard_name.to_string(),
            title: dashboard.title,
            uid: dashboard.uid,
            description: dashboard.description,
            tags: dashboard.tags,
            panel_count: dashboard.panels.len(),
            refresh_rate: dashboard.refresh,
            time_range: dashboard.time,
        })
    }

    /// Create a prometheus data source configuration
    pub fn create_prometheus_datasource(&self, prometheus_url: &str) -> GrafanaDataSource {
        GrafanaDataSource {
            name: "Pulzr Prometheus".to_string(),
            r#type: "prometheus".to_string(),
            url: prometheus_url.to_string(),
            access: "proxy".to_string(),
            is_default: true,
        }
    }

    /// Generate import instructions for a dashboard
    pub fn generate_import_instructions(&self, dashboard_name: &str) -> Result<String> {
        let info = self.get_dashboard_info(dashboard_name)?;

        #[allow(clippy::format_in_format_args)]
        let instructions = format!(
            r#"# Import Instructions for {dashboard_name}

## Dashboard Information
- **Title**: {title}
- **UID**: {uid}
- **Description**: {description}
- **Tags**: {tags}
- **Panels**: {panel_count}
- **Refresh Rate**: {refresh_rate}

## Steps to Import

1. **Enable Prometheus in Pulzr**:
   ```bash
   pulzr --url https://httpbin.org/get --prometheus --prometheus-port 9090
   ```

2. **Configure Prometheus Data Source**:
   - Go to Configuration → Data Sources in Grafana
   - Add new Prometheus data source
   - URL: `http://localhost:9090`
   - Save & Test

3. **Import Dashboard**:
   - Go to Create → Import in Grafana
   - Upload: `{dashboard_file}`
   - Select Prometheus data source
   - Save

## Dashboard Features
- Real-time metrics with {refresh_rate} refresh
- Time range: {time_from} to {time_to}
- {panel_count} visualization panels
- Optimized for load testing analysis

## Required Metrics
This dashboard requires the following Pulzr metrics:
- pulzr_requests_total
- pulzr_requests_successful_total
- pulzr_requests_failed_total
- pulzr_requests_per_second
- pulzr_response_time_p*_ms
- pulzr_request_duration_seconds
- pulzr_active_connections
- pulzr_bytes_received_total
- pulzr_error_rate_percent

## Troubleshooting
- Verify Prometheus endpoint is accessible
- Check that Pulzr is running with --prometheus flag
- Confirm data source configuration in Grafana
"#,
            dashboard_name = dashboard_name,
            title = info.title,
            uid = info.uid,
            description = info.description,
            tags = info.tags.join(", "),
            panel_count = info.panel_count,
            refresh_rate = info.refresh_rate,
            dashboard_file = format!("{}.json", dashboard_name),
            time_from = info.time_range.from,
            time_to = info.time_range.to,
        );

        Ok(instructions)
    }

    /// Validate dashboard file
    pub fn validate_dashboard(&self, dashboard_name: &str) -> Result<Vec<String>> {
        let dashboard = self.load_dashboard(dashboard_name)?;
        let mut issues = Vec::new();

        // Check required fields
        if dashboard.title.is_empty() {
            issues.push("Dashboard title is empty".to_string());
        }

        if dashboard.uid.is_empty() {
            issues.push("Dashboard UID is empty".to_string());
        }

        if dashboard.panels.is_empty() {
            issues.push("Dashboard has no panels".to_string());
        }

        // Check for common prometheus queries
        let dashboard_str = serde_json::to_string(&dashboard)?;
        if !dashboard_str.contains("pulzr_") {
            issues.push("Dashboard doesn't seem to contain Pulzr metrics".to_string());
        }

        // Check time range
        if dashboard.time.from.is_empty() || dashboard.time.to.is_empty() {
            issues.push("Time range is not properly configured".to_string());
        }

        Ok(issues)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardInfo {
    pub name: String,
    pub title: String,
    pub uid: String,
    pub description: String,
    pub tags: Vec<String>,
    pub panel_count: usize,
    pub refresh_rate: String,
    pub time_range: TimeRange,
}

impl DashboardInfo {
    pub fn print_summary(&self) {
        println!("📊 Dashboard: {}", self.name);
        println!("   Title: {}", self.title);
        println!("   UID: {}", self.uid);
        println!("   Description: {}", self.description);
        println!("   Tags: {}", self.tags.join(", "));
        println!("   Panels: {}", self.panel_count);
        println!("   Refresh: {}", self.refresh_rate);
        println!(
            "   Time Range: {} to {}",
            self.time_range.from, self.time_range.to
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_grafana_manager_creation() {
        let manager = GrafanaManager::new(None);
        assert_eq!(manager.dashboards_dir, "dashboards");

        let manager = GrafanaManager::new(Some("custom_dir".to_string()));
        assert_eq!(manager.dashboards_dir, "custom_dir");
    }

    #[test]
    fn test_create_prometheus_datasource() {
        let manager = GrafanaManager::new(None);
        let datasource = manager.create_prometheus_datasource("http://localhost:9090");

        assert_eq!(datasource.name, "Pulzr Prometheus");
        assert_eq!(datasource.r#type, "prometheus");
        assert_eq!(datasource.url, "http://localhost:9090");
        assert_eq!(datasource.access, "proxy");
        assert!(datasource.is_default);
    }

    #[test]
    fn test_dashboard_validation() {
        let temp_dir = TempDir::new().unwrap();
        let dashboards_dir = temp_dir.path().join("dashboards");
        fs::create_dir_all(&dashboards_dir).unwrap();

        // Create a minimal valid dashboard
        let dashboard_content = r#"{
            "title": "Test Dashboard",
            "uid": "test-dashboard",
            "version": 1,
            "description": "Test dashboard for validation",
            "tags": ["test"],
            "refresh": "5s",
            "time": {
                "from": "now-5m",
                "to": "now"
            },
            "panels": [
                {
                    "id": 1,
                    "title": "Test Panel",
                    "type": "timeseries",
                    "targets": [
                        {
                            "expr": "pulzr_requests_total"
                        }
                    ]
                }
            ],
            "templating": {
                "list": []
            }
        }"#;

        let dashboard_path = dashboards_dir.join("test-dashboard.json");
        fs::write(&dashboard_path, dashboard_content).unwrap();

        let manager = GrafanaManager::new(Some(dashboards_dir.to_string_lossy().to_string()));
        let issues = manager.validate_dashboard("test-dashboard").unwrap();

        assert!(issues.is_empty(), "Dashboard should be valid: {issues:?}");
    }

    #[test]
    fn test_list_dashboards() {
        let temp_dir = TempDir::new().unwrap();
        let dashboards_dir = temp_dir.path().join("dashboards");
        fs::create_dir_all(&dashboards_dir).unwrap();

        // Create test dashboard files
        fs::write(dashboards_dir.join("dashboard1.json"), "{}").unwrap();
        fs::write(dashboards_dir.join("dashboard2.json"), "{}").unwrap();
        fs::write(dashboards_dir.join("not-a-dashboard.txt"), "{}").unwrap();

        let manager = GrafanaManager::new(Some(dashboards_dir.to_string_lossy().to_string()));
        let dashboards = manager.list_dashboards().unwrap();

        assert_eq!(dashboards.len(), 2);
        assert!(dashboards.contains(&"dashboard1".to_string()));
        assert!(dashboards.contains(&"dashboard2".to_string()));
        assert!(!dashboards.contains(&"not-a-dashboard".to_string()));
    }
}
