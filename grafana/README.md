# DBPulse Grafana Dashboard

This directory contains the Grafana dashboard for monitoring DBPulse metrics.

## Installation

1. Open Grafana
2. Navigate to Dashboards > Import
3. Upload `dashboard.json` or paste its contents
4. Select your Prometheus datasource
5. Click Import

## Quick Start

The dashboard provides comprehensive monitoring of database health, performance, and operational metrics:

- **Health Status** - Real-time database availability
- **Performance Metrics** - Latency breakdown by operation
- **Error Tracking** - Detailed error classification
- **Connection Monitoring** - Active connections and lifecycle
- **Resource Usage** - Table size and row count tracking
- **TLS Security** - Handshake performance and error tracking

---

## Complete Metrics Reference

### Core Health Metrics

#### `dbpuse_pulse` (Gauge)
**Description:** Binary health indicator
- `1` = Database is healthy (read/write operations successful)
- `0` = Database is unhealthy (errors detected)

**Use Case:** Primary health signal for alerts

**Query Examples:**
```promql
# Current health status
dbpuse_pulse

# Uptime percentage over last 24h
avg_over_time(dbpuse_pulse[24h]) * 100
```

#### `dbpulse_runtime` (Histogram)
**Description:** Total duration of complete health check cycle (in seconds)

**Buckets:** Default Prometheus histogram buckets

**Query Examples:**
```promql
# Average latency
rate(dbpulse_runtime_sum[5m]) / rate(dbpulse_runtime_count[5m])

# P99 latency
histogram_quantile(0.99, rate(dbpulse_runtime_bucket[5m]))

# P95 latency
histogram_quantile(0.95, rate(dbpulse_runtime_bucket[5m]))

# P50 latency (median)
histogram_quantile(0.50, rate(dbpulse_runtime_bucket[5m]))
```

#### `dbpulse_database_version_info` (Gauge)
**Description:** Exposes the reported database version as a label (`version`) with a constant value of `1`.

**Use Case:** Surface the current DB engine version in Stat panels or annotations.

**Query Examples:**
```promql
# Show the latest version string (Grafana Stat panel title can use {{version}})
dbpulse_database_version_info{database="postgres"}
```

#### `dbpulse_database_uptime_seconds` (Gauge)
**Description:** How long (in seconds) the database has been up (`pg_postmaster_start_time` or `SHOW GLOBAL STATUS LIKE 'Uptime'`).

**Query Examples:**
```promql
# Current uptime in hours
dbpulse_database_uptime_seconds{database="mysql"} / 3600

# Alert if DB restarted in last 10 minutes
dbpulse_database_uptime_seconds < 600
```

---

### Error Classification Metrics

#### `dbpulse_errors_total` (Counter)
**Labels:** `database` (postgres/mysql), `error_type`

**Error Types:**
- `authentication` - Invalid credentials or authentication failures
- `timeout` - Query timeouts
- `connection` - Connection establishment failures
- `transaction` - Transaction rollback or consistency errors
- `query` - SQL execution errors

**Query Examples:**
```promql
# Total error rate by type
rate(dbpulse_errors_total[5m])

# Authentication errors for PostgreSQL
rate(dbpulse_errors_total{database="postgres",error_type="authentication"}[5m])

# Error breakdown by type
sum by (error_type) (rate(dbpulse_errors_total[5m]))

# Connection error percentage
rate(dbpulse_errors_total{error_type="connection"}[5m]) /
  rate(dbpulse_iterations_total[5m]) * 100
```

---

### Operation Performance Metrics

#### `dbpulse_operation_duration_seconds` (Histogram)
**Labels:** `database` (postgres/mysql), `operation`

**Operations:**
- `connect` - Database connection establishment
- `create_table` - Table creation/verification
- `insert` - INSERT or UPSERT operations
- `select` - SELECT query verification
- `transaction_test` - Transaction rollback test
- `cleanup` - Old record deletion

**Query Examples:**
```promql
# Average duration by operation
rate(dbpulse_operation_duration_seconds_sum[5m]) /
  rate(dbpulse_operation_duration_seconds_count[5m])

# Slow INSERT operations (P99)
histogram_quantile(0.99,
  rate(dbpulse_operation_duration_seconds_bucket{operation="insert"}[5m]))

# Connection establishment time
rate(dbpulse_operation_duration_seconds_sum{operation="connect"}[5m]) /
  rate(dbpulse_operation_duration_seconds_count{operation="connect"}[5m])

# Cleanup operation duration (to detect slow DELETEs)
histogram_quantile(0.95,
  rate(dbpulse_operation_duration_seconds_bucket{operation="cleanup"}[5m]))
```

---

### Connection Lifecycle Metrics

#### `dbpulse_connections_active` (Gauge)
**Description:** Currently active database connections

**Expected Value:** Typically `0` or `1` (connections are opened and closed per iteration)

**Query Examples:**
```promql
# Current active connections
dbpulse_connections_active

# Alert if connection held too long
dbpulse_connections_active > 0
```

#### `dbpulse_connection_duration_seconds` (Histogram)
**Description:** Total time connection is held open

**Query Examples:**
```promql
# Average connection hold time
rate(dbpulse_connection_duration_seconds_sum[5m]) /
  rate(dbpulse_connection_duration_seconds_count[5m])

# P99 connection duration
histogram_quantile(0.99, rate(dbpulse_connection_duration_seconds_bucket[5m]))
```

---

### Data Modification Tracking

#### `dbpulse_rows_affected_total` (Counter)
**Labels:** `database` (postgres/mysql), `operation` (insert/delete)

**Description:** Total rows affected by write operations

**Query Examples:**
```promql
# Insert rate
rate(dbpulse_rows_affected_total{operation="insert"}[5m])

# Delete rate (cleanup)
rate(dbpulse_rows_affected_total{operation="delete"}[5m])

# Rows deleted per cleanup cycle
increase(dbpulse_rows_affected_total{operation="delete"}[1h])
```

---

### Iteration and Success Tracking

#### `dbpulse_iterations_total` (Counter)
**Labels:** `database` (postgres/mysql), `status` (success/error)

**Description:** Total monitoring iterations

**Query Examples:**
```promql
# Success rate
rate(dbpulse_iterations_total{status="success"}[5m]) /
  rate(dbpulse_iterations_total[5m]) * 100

# Error rate
rate(dbpulse_iterations_total{status="error"}[5m])

# Total iterations
sum(rate(dbpulse_iterations_total[5m]))
```

#### `dbpulse_last_success_timestamp_seconds` (Gauge)
**Labels:** `database` (postgres/mysql)

**Description:** Unix timestamp of last successful health check

**Query Examples:**
```promql
# Time since last success (in minutes)
(time() - dbpulse_last_success_timestamp_seconds) / 60

# Alert if no success in 5 minutes
time() - dbpulse_last_success_timestamp_seconds > 300
```

---

### Table Size Monitoring

#### `dbpulse_table_size_bytes` (Gauge)
**Labels:** `database` (postgres/mysql), `table`

**Description:** Approximate table size in bytes

**Query Examples:**
```promql
# Current table size in MB
dbpulse_table_size_bytes / 1024 / 1024

# Table growth rate (bytes per second)
rate(dbpulse_table_size_bytes[1h])

# Alert if table growing too fast
rate(dbpulse_table_size_bytes[1h]) > 1000000  # 1MB/s
```

#### `dbpulse_table_rows` (Gauge)
**Labels:** `database` (postgres/mysql), `table`

**Description:** Approximate row count

**Query Examples:**
```promql
# Current row count
dbpulse_table_rows

# Row growth rate
rate(dbpulse_table_rows[1h])

# Alert if cleanup not working (unbounded growth)
rate(dbpulse_table_rows[1h]) > 1000
```

---

### Reliability Metrics

#### `dbpulse_panics_recovered_total` (Counter)
**Description:** Total panics recovered from in monitoring loop

**Query Examples:**
```promql
# Panic rate
rate(dbpulse_panics_recovered_total[5m])

# Total panics in last 24h
increase(dbpulse_panics_recovered_total[24h])
```

#### `dbpulse_database_readonly` (Gauge)
**Labels:** `database` (postgres/mysql)

**Description:** Database read-only status
- `1` = Database in read-only mode (recovery/replica)
- `0` = Database in read-write mode

**Query Examples:**
```promql
# Current read-only status
dbpulse_database_readonly

# Alert on failover/replica promotion
changes(dbpulse_database_readonly[5m]) > 0
```

---

### TLS/SSL Metrics

#### `dbpulse_tls_handshake_duration_seconds` (Histogram)
**Labels:** `database` (postgres/mysql)

**Description:** TLS handshake duration (connection establishment time when TLS enabled)

**Query Examples:**
```promql
# Average TLS handshake time
rate(dbpulse_tls_handshake_duration_seconds_sum[5m]) /
  rate(dbpulse_tls_handshake_duration_seconds_count[5m])

# P99 TLS handshake
histogram_quantile(0.99,
  rate(dbpulse_tls_handshake_duration_seconds_bucket[5m]))
```

#### `dbpulse_tls_connection_errors_total` (Counter)
**Labels:** `database` (postgres/mysql), `error_type` (handshake)

**Description:** TLS-specific connection errors

**Query Examples:**
```promql
# TLS error rate
rate(dbpulse_tls_connection_errors_total[5m])

# TLS errors by database
sum by (database) (rate(dbpulse_tls_connection_errors_total[5m]))
```

#### `dbpulse_tls_info` (Gauge)
**Labels:** `database` (postgres/mysql), `version` (TLSv1.2/TLSv1.3), `cipher`

**Description:** TLS connection information (value always `1`)

**Query Examples:**
```promql
# Current TLS version
dbpulse_tls_info

# TLS version breakdown
sum by (version) (dbpulse_tls_info)
```

#### `dbpulse_tls_cert_expiry_days` (Gauge)
**Labels:** `database` (postgres/mysql)

**Description:** Days until TLS certificate expiration (negative if expired). Only available for MySQL/MariaDB with TLS enabled.

**Use Case:** Proactive certificate lifecycle management and expiration alerting

**Query Examples:**
```promql
# Current certificate expiry status
dbpulse_tls_cert_expiry_days

# Certificates expiring within 30 days
dbpulse_tls_cert_expiry_days < 30

# Certificates already expired
dbpulse_tls_cert_expiry_days < 0

# Days until next certificate renewal needed
min(dbpulse_tls_cert_expiry_days)
```

**Alert Thresholds:**
- **Critical (<7 days):** Renew immediately to prevent outage
- **Warning (<30 days):** Plan certificate renewal
- **Expired (<0 days):** Certificate has expired, connections may fail

**Note:** PostgreSQL's `pg_stat_ssl` doesn't expose certificate metadata. For PostgreSQL, monitor certificate files externally.

---

## Alert Rules

### Critical Alerts

#### Database Down
```yaml
- alert: DatabaseDown
  expr: dbpuse_pulse == 0
  for: 2m
  labels:
    severity: critical
  annotations:
    summary: "Database is down"
    description: "DBPulse reports database unhealthy for 2 minutes"
```

#### Database Check Stale
```yaml
- alert: DatabaseCheckStale
  expr: time() - dbpulse_last_success_timestamp_seconds > 300
  for: 1m
  labels:
    severity: critical
  annotations:
    summary: "No successful database check in 5 minutes"
    description: "Last success: {{ $value | humanizeDuration }}"
```

#### Connection Leak Suspected
```yaml
- alert: ConnectionLeakSuspected
  expr: dbpulse_connections_active > 0
  for: 1m
  labels:
    severity: warning
  annotations:
    summary: "Database connection held for >1 minute"
    description: "Possible connection leak detected"
```

### Warning Alerts

#### High Error Rate
```yaml
- alert: DatabaseErrorRateHigh
  expr: rate(dbpulse_errors_total[5m]) > 0.1
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "High database error rate"
    description: "Error rate: {{ $value | humanize }}/s"
```

#### Connection Errors
```yaml
- alert: DatabaseConnectionErrors
  expr: rate(dbpulse_errors_total{error_type="connection"}[5m]) > 0.05
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "Database connection errors detected"
    description: "{{ $labels.database }} connection error rate: {{ $value | humanize }}/s"
```

#### Slow Database Operations
```yaml
- alert: SlowDatabaseInserts
  expr: |
    histogram_quantile(0.95,
      rate(dbpulse_operation_duration_seconds_bucket{operation="insert"}[5m])
    ) > 1
  for: 10m
  labels:
    severity: warning
  annotations:
    summary: "Database inserts are slow"
    description: "P95 insert latency: {{ $value | humanizeDuration }}"
```

#### High Overall Latency
```yaml
- alert: DatabaseHighLatency
  expr: histogram_quantile(0.99, rate(dbpulse_runtime_bucket[5m])) > 1
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Database health check latency is high"
    description: "P99 latency: {{ $value | humanizeDuration }}"
```

#### Table Growth Unbounded
```yaml
- alert: TableGrowthUnbounded
  expr: rate(dbpulse_table_rows[1h]) > 1000
  for: 30m
  labels:
    severity: warning
  annotations:
    summary: "dbpulse table growing rapidly"
    description: "Cleanup may be failing. Growth rate: {{ $value | humanize }} rows/s"
```

#### TLS Errors
```yaml
- alert: DBPulseTLSErrors
  expr: rate(dbpulse_tls_connection_errors_total[5m]) > 0
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "TLS connection errors detected"
    description: "{{ $labels.database }} TLS errors: {{ $value | humanize }}/s"
```

#### TLS Certificate Expiring Soon (Critical)
```yaml
- alert: TLSCertificateExpiringSoon
  expr: dbpulse_tls_cert_expiry_days < 7
  for: 1h
  labels:
    severity: critical
  annotations:
    summary: "TLS certificate expires in less than 7 days"
    description: "{{ $labels.database }} certificate expires in {{ $value }} days - RENEW IMMEDIATELY"
```

#### TLS Certificate Expiring (Warning)
```yaml
- alert: TLSCertificateExpiringWarning
  expr: dbpulse_tls_cert_expiry_days < 30
  for: 1h
  labels:
    severity: warning
  annotations:
    summary: "TLS certificate expires in less than 30 days"
    description: "{{ $labels.database }} certificate expires in {{ $value }} days - plan renewal"
```

#### TLS Certificate Expired
```yaml
- alert: TLSCertificateExpired
  expr: dbpulse_tls_cert_expiry_days < 0
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "TLS certificate has EXPIRED"
    description: "{{ $labels.database }} certificate expired {{ $value | humanize }} days ago"
```

#### Monitoring Loop Panics
```yaml
- alert: MonitoringLoopPanics
  expr: rate(dbpulse_panics_recovered_total[5m]) > 0
  for: 5m
  labels:
    severity: warning
  annotations:
    summary: "Monitoring loop experiencing panics"
    description: "Panic rate: {{ $value | humanize }}/s"
```

---

## Dashboard Customization

### Adding New Panels

1. Enter dashboard edit mode
2. Click "Add" > "Visualization"
3. Select metric from the list above
4. Choose visualization type:
   - **Gauge** - Single value (health status, current connections)
   - **Time Series** - Values over time (latency, error rates)
   - **Bar Chart** - Comparisons (error types, operations)
   - **Stat** - Single number with trends
   - **Table** - Multiple metrics together

### Useful Panel Combinations

#### Database Health Overview
```promql
# Create a stat panel with multiple queries
1. dbpuse_pulse (current health)
2. rate(dbpulse_iterations_total{status="success"}[5m]) / rate(dbpulse_iterations_total[5m]) * 100 (success rate)
3. histogram_quantile(0.99, rate(dbpulse_runtime_bucket[5m])) (P99 latency)
```

#### Error Breakdown
```promql
# Create a pie chart or bar gauge
sum by (error_type) (rate(dbpulse_errors_total[5m]))
```

#### Operation Performance Comparison
```promql
# Create a time series with multiple queries
rate(dbpulse_operation_duration_seconds_sum{operation="connect"}[5m]) /
  rate(dbpulse_operation_duration_seconds_count{operation="connect"}[5m])

rate(dbpulse_operation_duration_seconds_sum{operation="insert"}[5m]) /
  rate(dbpulse_operation_duration_seconds_count{operation="insert"}[5m])

rate(dbpulse_operation_duration_seconds_sum{operation="select"}[5m]) /
  rate(dbpulse_operation_duration_seconds_count{operation="select"}[5m])
```

---

## Troubleshooting

### No Data Showing
1. Verify DBPulse is running: `curl http://localhost:8080/metrics`
2. Check Prometheus is scraping: Check Prometheus targets page
3. Verify datasource in Grafana: Configuration > Data Sources
4. Check time range includes when DBPulse was running

### Missing Metrics
- **TLS Metrics:** Only available when DSN includes `sslmode` parameter (e.g., `?sslmode=require`)
- **Table Size Metrics:** Only recorded during periodic checks (minute 0 of each hour)
- **Panic Metrics:** Only incremented when panics actually occur

### Incorrect Values
- Verify Prometheus scrape interval matches expectations
- Check DBPulse `--interval` configuration (default 30s)
- Ensure rate() intervals are at least 2x scrape interval
- Check data retention in Prometheus

### High Memory Usage in Grafana
- Reduce dashboard refresh rate (default: 10s)
- Limit time range (default: 1h)
- Use recording rules in Prometheus for complex queries

---

## Best Practices

### Query Performance
- Use `rate()` for counters, not `increase()`
- Keep rate intervals at least 2x scrape interval
- Use recording rules for frequently used complex queries
- Limit cardinality by avoiding high-cardinality label combinations

### Dashboard Organization
- Group related panels together
- Use consistent time ranges across panels
- Add descriptions to complex panels
- Use panel links to related dashboards

### Alerting
- Set appropriate `for` durations to avoid flapping
- Use multiple severity levels (critical/warning)
- Include runbook links in annotations
- Test alerts in non-production first

---

## Recording Rules

For better performance, pre-calculate frequently used queries:

```yaml
groups:
  - name: dbpulse_rules
    interval: 30s
    rules:
      # Success rate
      - record: dbpulse:success_rate
        expr: |
          rate(dbpulse_iterations_total{status="success"}[5m]) /
          rate(dbpulse_iterations_total[5m]) * 100

      # Average latency
      - record: dbpulse:latency:avg
        expr: |
          rate(dbpulse_runtime_sum[5m]) /
          rate(dbpulse_runtime_count[5m])

      # P99 latency
      - record: dbpulse:latency:p99
        expr: histogram_quantile(0.99, rate(dbpulse_runtime_bucket[5m]))

      # Error rate by type
      - record: dbpulse:errors:rate
        expr: rate(dbpulse_errors_total[5m])
```

---

## Configuration

### Templating Variables
The dashboard supports the following variables:
- `datasource` - Prometheus datasource selection
- `database` - Filter by database type (postgres/mysql)
- `interval` - Query interval (auto, 1m, 5m, 10m, 30m, 1h)

### Default Settings
- **Time Range:** Last 1 hour
- **Refresh:** 10 seconds
- **Timezone:** Browser local time

### Recommended Settings
- **Production:** 30s refresh, 6h time range
- **Development:** 5s refresh, 1h time range
- **Troubleshooting:** 5s refresh, 15m time range

---

## Integration

### With Alertmanager
```yaml
# prometheus.yml
alerting:
  alertmanagers:
    - static_configs:
        - targets:
            - alertmanager:9093

rule_files:
  - /etc/prometheus/dbpulse_alerts.yml
```

### With Other Dashboards
Create dashboard links to:
- Application performance monitoring (APM)
- Infrastructure monitoring
- Log aggregation (Loki/ELK)
- Distributed tracing (Jaeger/Tempo)

---

## Support

For issues or questions:
- GitHub Issues: https://github.com/nbari/dbpulse/issues
- Documentation: https://github.com/nbari/dbpulse/blob/master/README.md
