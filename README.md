[![build](https://github.com/nbari/dbpulse/actions/workflows/build.yml/badge.svg)](https://github.com/nbari/dbpulse/actions/workflows/build.yml)
[![codecov](https://codecov.io/gh/nbari/dbpulse/graph/badge.svg?token=I7X5VOMML6)](https://codecov.io/gh/nbari/dbpulse)
[![crates.io](https://img.shields.io/crates/v/dbpulse.svg)](https://crates.io/crates/dbpulse)

# dbpulse

`dbpulse` will run a set of queries in a defined interval, in order to
dynamically test if the database is available mainly for writes, it exposes a
`/metrics` endpoint the one can be used together with `Prometheus` and create
alerts when the database is not available, this is to cover HALT/LOCK cases in
Galera clusters in where a `DDL` could stale the whole cluster or flow-control
kicks in and the database could not be receiving `COMMITS/WRITE`.

## How to use it

Run it as a client, probably hitting your load balancer so that you can test
like if you where a client, you need to pass the `DSN` or see it up as an
environment var.

## Metrics

dbpulse exposes comprehensive Prometheus-compatible metrics on the `/metrics` endpoint.

### Quick Reference

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_pulse` | Gauge | Health status (1=ok, 0=error) |
| `dbpulse_runtime` | Histogram | Total operation latency |
| `dbpulse_errors_total` | Counter | Errors by type (auth, timeout, connection, etc.) |
| `dbpulse_operation_duration_seconds` | Histogram | Per-operation timing breakdown |
| `dbpulse_connections_active` | Gauge | Currently active connections |
| `dbpulse_iterations_total` | Counter | Success/error iteration counts |
| `dbpulse_last_success_timestamp_seconds` | Gauge | Last successful check timestamp |
| `dbpulse_rows_affected_total` | Counter | Rows affected by operations |
| `dbpulse_table_size_bytes` | Gauge | Table size in bytes |
| `dbpulse_database_readonly` | Gauge | Read-only mode (1=yes, 0=no) |
| `dbpulse_tls_handshake_duration_seconds` | Histogram | TLS handshake timing |
| `dbpulse_tls_info` | Gauge | TLS version and cipher info |

For complete documentation, PromQL examples, and alert rules, see [grafana/README.md](grafana/README.md).

### Key Metrics Examples

```promql
# Database health
dbpulse_pulse

# Success rate
rate(dbpulse_iterations_total{status="success"}[5m]) /
  rate(dbpulse_iterations_total[5m]) * 100

# P99 latency
histogram_quantile(0.99, rate(dbpulse_runtime_bucket[5m]))

# Error rate by type
rate(dbpulse_errors_total[5m])

# Connection time
rate(dbpulse_operation_duration_seconds_sum{operation="connect"}[5m]) /
  rate(dbpulse_operation_duration_seconds_count{operation="connect"}[5m])
```

### Example Alerts

```yaml
- alert: DatabaseDown
  expr: dbpulse_pulse == 0
  for: 2m
  labels:
    severity: critical

- alert: HighErrorRate
  expr: rate(dbpulse_errors_total[5m]) > 0.1
  for: 5m
  labels:
    severity: warning

- alert: NoRecentSuccess
  expr: time() - dbpulse_last_success_timestamp_seconds > 300
  for: 1m
  labels:
    severity: critical
```


Current options:

```
command line tool to monitor that database is available for read & write

Usage: dbpulse [OPTIONS] --dsn <dsn>

Options:
  -d, --dsn <dsn>            <mysql|postgres>://<username>:<password>@tcp(<host>:<port>)/<database> [env: DBPULSE_DSN=postgres://postgres:secret@tcp(localhost)/dbpulse]
  -i, --interval <interval>  number of seconds between checks [env: DBPULSE_INTERVAL=] [default: 30]
  -p, --port <port>          listening port for /metrics [env: DBPULSE_PORT=] [default: 9300]
  -r, --range <range>        The upper limit of the ID range [env: DBPULSE_RANGE=] [default: 100]
  -h, --help                 Print help
  -V, --version              Print version

```

Example:

```sh
dbpulse --dsn "postgres://postgres:secret@tcp(10.10.0.10)/dbpulse" -r 2880
```

> the app tries to create the database if it does not exist (depends on the user permissions)

## Container Image

Container images are automatically published to GitHub Container Registry (GHCR) on each release.

### Pull the image

```sh
podman pull ghcr.io/nbari/dbpulse:latest
```

### Run with Podman

```sh
# PostgreSQL
podman run -p 9300:9300 ghcr.io/nbari/dbpulse:latest \
  --dsn "postgres://user:password@host:5432/dbname" \
  --listen "0.0.0.0:9300"

# MySQL/MariaDB
podman run -p 9300:9300 ghcr.io/nbari/dbpulse:latest \
  --dsn "mysql://user:password@tcp(host:3306)/dbname" \
  --listen "0.0.0.0:9300"
```

### Multi-architecture support

Images are built for:
- `linux/amd64` - x86_64 architecture
- `linux/arm64` - ARM64 architecture (AWS Graviton, Raspberry Pi, etc.)
