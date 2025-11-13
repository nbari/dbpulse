# DBPulse Grafana Dashboard

This directory contains the Grafana dashboard for monitoring DBPulse metrics.

## Installation

1. Open Grafana
2. Navigate to Dashboards > Import
3. Upload `dashboard.json` or paste its contents
4. Select your Prometheus datasource
5. Click Import

## Dashboard Panels

### 1. Database Health Status
**Type:** Gauge
**Metric:** `dbpuse_pulse`
**Description:** Current health status of the database
- Green (1) = Database is healthy
- Red (0) = Database is experiencing errors

### 2. Database Pulse Latency
**Type:** Time Series
**Metrics:**
- `dbpulse_runtime_sum` / `dbpulse_runtime_count` (Average)
- `histogram_quantile(0.99, dbpulse_runtime_bucket)` (P99)
- `histogram_quantile(0.95, dbpulse_runtime_bucket)` (P95)
- `histogram_quantile(0.50, dbpulse_runtime_bucket)` (P50)

**Description:** Database health check latency over time, showing:
- Average latency
- P99 (99th percentile) - worst case scenarios
- P95 (95th percentile) - typical worst case
- P50 (50th percentile) - median latency

### 3. TLS Handshake Duration
**Type:** Time Series
**Metric:** `dbpulse_tls_handshake_duration_seconds`
**Labels:** `database` (postgres, mysql)
**Description:** TLS/SSL handshake duration per database type
- Average handshake time
- P99 handshake time
- Useful for detecting SSL performance issues

### 4. TLS Connection Errors Rate
**Type:** Time Series (Stacked)
**Metric:** `dbpulse_tls_connection_errors_total`
**Labels:** `database`, `error_type`
**Description:** Rate of TLS connection errors over time
- Stacked view shows total error rate
- Broken down by database and error type
- Useful for detecting intermittent SSL handshake failures

### 5. TLS Connection Info
**Type:** Bar Gauge
**Metric:** `dbpulse_tls_info`
**Labels:** `database`, `version`, `cipher`
**Description:** Current TLS connection information
- Shows active TLS version (TLSv1.2, TLSv1.3, etc.)
- Shows cipher suite in use
- Broken down by database type

### 6. TLS Connection Errors Total
**Type:** Bar Gauge
**Metric:** `dbpulse_tls_connection_errors_total`
**Labels:** `database`, `error_type`
**Description:** Total count of TLS connection errors
- Cumulative error count
- Broken down by database and error type

### 7. Health Check Rate
**Type:** Time Series
**Metric:** `rate(dbpulse_runtime_count[5m])`
**Description:** Rate of health checks per second
- Shows how frequently DBPulse is checking the database
- Should match configured interval

## Metrics Reference

### Core Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `dbpuse_pulse` | Gauge | Health status (1=ok, 0=error) |
| `dbpulse_runtime` | Histogram | Health check latency in seconds |
| `dbpulse_runtime_bucket` | Histogram Bucket | Latency distribution buckets |
| `dbpulse_runtime_sum` | Counter | Total latency sum |
| `dbpulse_runtime_count` | Counter | Total health checks |

### TLS Metrics

| Metric | Type | Labels | Description |
|--------|------|--------|-------------|
| `dbpulse_tls_handshake_duration_seconds` | Histogram | `database` | TLS handshake duration |
| `dbpulse_tls_connection_errors_total` | Counter | `database`, `error_type` | TLS connection errors |
| `dbpulse_tls_info` | Gauge | `database`, `version`, `cipher` | TLS connection info |

## Alert Examples

### High Latency Alert
```yaml
- alert: DBPulseHighLatency
  expr: histogram_quantile(0.99, rate(dbpulse_runtime_bucket[5m])) > 1
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Database health check latency is high"
    description: "P99 latency is above 1 second for 5 minutes"
```

### Database Down Alert
```yaml
- alert: DBPulseDown
  expr: dbpuse_pulse == 0
  for: 1m
  labels:
    severity: critical
  annotations:
    summary: "Database is down"
    description: "DBPulse reports database is not healthy"
```

### TLS Handshake Errors Alert
```yaml
- alert: DBPulseTLSErrors
  expr: rate(dbpulse_tls_connection_errors_total[5m]) > 0
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "TLS connection errors detected"
    description: "Database {{$labels.database}} is experiencing TLS errors: {{$labels.error_type}}"
```

## Configuration

The dashboard uses a templated datasource variable. When importing:
- The `datasource` variable will auto-populate with available Prometheus datasources
- Select the appropriate datasource for your DBPulse metrics
- Refresh rate is set to 10 seconds by default
- Time range defaults to last 1 hour

## Customization

### Changing Time Range
Default: Last 1 hour
- Click time picker in top right
- Select desired range or set custom range

### Adjusting Refresh Rate
Default: 10 seconds
- Click refresh dropdown in top right
- Options: 5s, 10s, 30s, 1m, 5m, 15m, 30m, 1h, 2h, 1d

### Adding Custom Panels
The dashboard includes all DBPulse metrics. To add custom panels:
1. Click "Add panel" in dashboard edit mode
2. Use any of the metrics listed above
3. Configure visualization type and options
4. Save dashboard

## Troubleshooting

### No Data Showing
- Verify DBPulse is running and exposing metrics on `/metrics` endpoint
- Check Prometheus is scraping DBPulse metrics endpoint
- Verify datasource is correctly configured in Grafana
- Check time range includes period when DBPulse was running

### Missing TLS Metrics
- TLS metrics only appear when `--tls-mode` is not `disable`
- Verify DBPulse is configured with TLS enabled
- Check that database supports TLS connections

### Incorrect Values
- Verify Prometheus scrape interval matches expected values
- Check DBPulse `--interval` configuration
- Ensure time range is appropriate for data retention
