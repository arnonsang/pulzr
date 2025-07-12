# Pulzr Grafana Dashboards

This directory contains pre-built Grafana dashboards for visualizing Pulzr load testing metrics.

## Available Dashboards

### 1. Pulzr Overview (`pulzr-overview.json`)
**Purpose**: High-level overview of load testing performance
- **Requests Per Second**: Real-time RPS monitoring
- **Error Rate Gauge**: Current error percentage with visual thresholds
- **Response Time Percentiles**: P50, P90, P95, P99 response times
- **Request Count**: 5-minute rate of total, successful, and failed requests
- **Active Connections**: Current number of active connections
- **HTTP Status Codes**: Distribution of response status codes

### 2. Pulzr Detailed Analysis (`pulzr-detailed.json`)
**Purpose**: In-depth analysis with advanced metrics
- **Request Duration Histogram**: Percentiles calculated from histogram data
- **Request Duration Heatmap**: Visual representation of response time distribution
- **Bytes Received Rate**: Data throughput monitoring
- **Request Rate Comparison**: Multiple time windows (1m, 5m rates)
- **Calculated Error Rate**: Dynamically calculated error percentage
- **Current Metrics Table**: Real-time values in tabular format

## Setup Instructions

### Prerequisites
- Grafana instance (version 8.0+)
- Prometheus data source configured
- Pulzr running with `--prometheus` flag

### Installation Steps

1. **Enable Prometheus in Pulzr**:
   ```bash
   pulzr --url https://httpbin.org/get --prometheus --prometheus-port 9090
   ```

2. **Configure Prometheus Data Source in Grafana**:
   - Go to Configuration → Data Sources
   - Add new Prometheus data source
   - URL: `http://localhost:9090` (or your Prometheus server)
   - Save & Test

3. **Import Dashboards**:
   - Go to Create → Import
   - Upload the JSON files from this directory
   - Select your Prometheus data source
   - Save the dashboard

### Dashboard Configuration

#### Refresh Rate
- Both dashboards are configured for 5-second refresh
- Adjust as needed for your monitoring requirements

#### Time Range
- Default: Last 5 minutes
- Modify based on your test duration

#### Templating
- Basic setup with room for expansion
- Can add variables for multiple test environments

## Metrics Reference

### Available Prometheus Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `pulzr_requests_total` | Counter | Total number of requests sent |
| `pulzr_requests_successful_total` | Counter | Number of successful requests |
| `pulzr_requests_failed_total` | Counter | Number of failed requests |
| `pulzr_requests_per_second` | Gauge | Current requests per second |
| `pulzr_request_duration_seconds` | Histogram | Request duration distribution |
| `pulzr_response_time_p50_ms` | Gauge | 50th percentile response time |
| `pulzr_response_time_p90_ms` | Gauge | 90th percentile response time |
| `pulzr_response_time_p95_ms` | Gauge | 95th percentile response time |
| `pulzr_response_time_p99_ms` | Gauge | 99th percentile response time |
| `pulzr_active_connections` | Gauge | Number of active connections |
| `pulzr_bytes_received_total` | Counter | Total bytes received |
| `pulzr_error_rate_percent` | Gauge | Current error rate percentage |
| `pulzr_status_code_*_total` | Counter | Requests by status code |

### Useful PromQL Queries

```promql
# Request rate over 5 minutes
rate(pulzr_requests_total[5m])

# Error rate calculation
(rate(pulzr_requests_failed_total[5m]) / rate(pulzr_requests_total[5m])) * 100

# 95th percentile from histogram
histogram_quantile(0.95, rate(pulzr_request_duration_seconds_bucket[5m]))

# Throughput in bytes per second
rate(pulzr_bytes_received_total[5m])
```

## Customization

### Adding New Panels
1. Edit the JSON file directly
2. Or use Grafana UI to modify and export
3. Update the `version` field when making changes

### Color Schemes
- Dashboards use Grafana's palette-classic theme
- Modify `color.mode` in panel configurations
- Dark theme optimized

### Thresholds
- Error rate gauge has configurable thresholds
- Modify `thresholds.steps` for custom alerts
- Green < 80% errors, Red ≥ 80% errors (adjust as needed)

## Troubleshooting

### Common Issues

1. **No Data Showing**:
   - Verify Prometheus endpoint is accessible
   - Check that Pulzr is running with `--prometheus` flag
   - Confirm data source configuration

2. **Metrics Not Updating**:
   - Check dashboard refresh rate
   - Verify time range covers active test period
   - Ensure Prometheus scraping is working

3. **Panel Errors**:
   - Validate PromQL queries in Prometheus directly
   - Check metric names match exactly
   - Verify data source is selected correctly

### Debug Commands

```bash
# Check Prometheus metrics endpoint
curl http://localhost:9090/metrics

# Test specific metric
curl http://localhost:9090/metrics | grep pulzr_requests_total

# Validate PromQL query
curl 'http://localhost:9090/api/v1/query?query=pulzr_requests_total'
```

## Dashboard Features

### Auto-refresh
- Dashboards automatically refresh every 5 seconds
- Disable for historical analysis
- Configurable per dashboard

### Responsive Design
- Optimized for various screen sizes
- Grid layout adapts to viewport
- Mobile-friendly panels

### Export Options
- Export as PNG/PDF for reports
- Share dashboard links
- Export panel data as CSV

## Best Practices

1. **Performance**:
   - Use appropriate time ranges
   - Limit refresh frequency for long tests
   - Consider data retention policies

2. **Monitoring**:
   - Set up alerts for critical thresholds
   - Monitor dashboard performance
   - Use annotations for test events

3. **Team Collaboration**:
   - Create dashboard folders
   - Use consistent naming conventions
   - Document custom modifications

## Version History

- v1.0: Initial dashboard release
- Supports Pulzr v0.1.0+ with Prometheus integration
- Compatible with Grafana 8.0+