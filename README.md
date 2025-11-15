[![Build Status](https://github.com/nbari/dbpulse/actions/workflows/build.yml/badge.svg)](https://github.com/nbari/dbpulse/actions/workflows/build.yml)
[![Test Status](https://github.com/nbari/dbpulse/actions/workflows/test.yml/badge.svg)](https://github.com/nbari/dbpulse/actions/workflows/test.yml)
[![Coverage](https://codecov.io/gh/nbari/dbpulse/graph/badge.svg?token=I7X5VOMML6)](https://codecov.io/gh/nbari/dbpulse)
[![Crates.io](https://img.shields.io/crates/v/dbpulse.svg)](https://crates.io/crates/dbpulse)
[![License](https://img.shields.io/crates/l/dbpulse.svg)](https://github.com/nbari/dbpulse/blob/master/LICENSE)
[![GHCR](https://ghcr-badge.egpl.dev/nbari/dbpulse/latest_tag?trim=major&label=latest)](https://github.com/nbari/dbpulse/pkgs/container/dbpulse)

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
  -d, --dsn <dsn>
          Database connection string with optional TLS parameters

          Format: <driver>://<user>:<pass>@tcp(<host>:<port>)/<db>?param1=value1&param2=value2

          TLS Parameters (query string):
          - sslmode: disable|require|verify-ca|verify-full (default: disable)
          - sslrootcert or sslca: Path to CA certificate file
          - sslcert: Path to client certificate file
          - sslkey: Path to client private key file

          Examples:
          postgres://user:pass@tcp(localhost:5432)/db?sslmode=require
          mysql://root:secret@tcp(db.example.com:3306)/prod?sslmode=verify-full&sslca=/etc/ssl/ca.crt

          [env: DBPULSE_DSN=]

  -i, --interval <interval>
          number of seconds between checks

          [env: DBPULSE_INTERVAL=]
          [default: 30]

  -l, --listen <IP>
          IP address to bind to (default: [::]:port, accepts both IPv6 and IPv4)

          [env: DBPULSE_LISTEN=]

  -p, --port <port>
          listening port for /metrics

          [env: DBPULSE_PORT=]
          [default: 9300]

  -r, --range <range>
          The upper limit of the ID range

          [env: DBPULSE_RANGE=]
          [default: 100]

  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version

```

Example:

```sh
dbpulse --dsn "postgres://postgres:secret@tcp(10.10.0.10)/dbpulse" -r 2880
```

> the app tries to create the database if it does not exist (depends on the user permissions)

## TLS/SSL Configuration

TLS parameters are configured directly in the DSN query string:

### TLS Modes

- `disable` - No TLS encryption (default)
- `require` - TLS required, no certificate verification
- `verify-ca` - Verify server certificate against CA
- `verify-full` - Verify certificate and hostname

### DSN Parameters

| Parameter | Description | Example |
|-----------|-------------|---------|
| `sslmode` | TLS mode | `?sslmode=require` |
| `sslrootcert` or `sslca` | Path to CA certificate | `&sslca=/etc/ssl/ca.crt` |
| `sslcert` | Path to client certificate | `&sslcert=/etc/ssl/client.crt` |
| `sslkey` | Path to client private key | `&sslkey=/etc/ssl/client.key` |

### Examples

**PostgreSQL with TLS (no verification):**
```sh
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/mydb?sslmode=require"
```

**PostgreSQL with full certificate verification:**
```sh
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/mydb?sslmode=verify-full&sslrootcert=/etc/ssl/certs/ca.crt"
```

**MySQL/MariaDB with TLS:**
```sh
dbpulse --dsn "mysql://user:pass@tcp(db.example.com:3306)/mydb?sslmode=require"
```

**MySQL with CA verification:**
```sh
dbpulse --dsn "mysql://user:pass@tcp(db.example.com:3306)/mydb?sslmode=verify-ca&sslca=/etc/ssl/ca.crt"
```

**With mutual TLS (client certificates):**
```sh
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/mydb?sslmode=verify-full&sslrootcert=/etc/ssl/ca.crt&sslcert=/etc/ssl/client.crt&sslkey=/etc/ssl/client.key"
```

### TLS Metrics

When TLS is enabled, dbpulse exposes additional metrics:
- `dbpulse_tls_handshake_duration_seconds` - TLS handshake timing
- `dbpulse_tls_connection_errors_total` - TLS-specific errors
- `dbpulse_tls_info` - TLS version and cipher information

See [grafana/README.md](grafana/README.md) for complete metrics documentation.

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

## Development

### Testing

**Run all tests (unit, integration, TLS):**
```bash
just test
```

**Run individual test suites:**
```bash
just unit-test         # Unit tests only
just test-integration  # Integration tests (non-TLS)
just test-tls          # TLS integration tests
```

For detailed documentation, see:
- [TLS_TESTING.md](TLS_TESTING.md) - TLS testing guide
- [scripts/README.md](scripts/README.md) - Script documentation
