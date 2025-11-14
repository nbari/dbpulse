[![build](https://github.com/nbari/dbpulse/actions/workflows/build.yml/badge.svg)](https://github.com/nbari/dbpulse/actions/workflows/build.yml)
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

dbpulse exposes Prometheus-compatible metrics on the `/metrics` endpoint to monitor database health, performance, and TLS connection status.

### Core Metrics

#### `dbpulse_pulse`
**Type:** Gauge
**Description:** Database health status indicator
- `1` - Database is healthy (read/write operations successful)
- `0` - Database is unhealthy (read/write operations failed)

**Use case:** Alert when database becomes unavailable for writes
```promql
# Alert when database is down
dbpulse_pulse == 0

# Alert when database has been down for 2 minutes
dbpulse_pulse == 0 and avg_over_time(dbpulse_pulse[2m]) < 0.5
```

#### `dbpulse_runtime`
**Type:** Histogram
**Description:** Latency of database read/write operations in seconds

**Use case:** Monitor database performance and detect slowdowns
```promql
# Average latency over 5 minutes
sum(rate(dbpulse_runtime_sum[5m])) / sum(rate(dbpulse_runtime_count[5m]))

# 95th percentile latency
histogram_quantile(0.95, rate(dbpulse_runtime_bucket[5m]))

# Alert when latency exceeds 1 second
sum(rate(dbpulse_runtime_sum[5m])) / sum(rate(dbpulse_runtime_count[5m])) > 1
```

### TLS Metrics

#### `dbpulse_tls_info`
**Type:** Gauge
**Description:** TLS connection information (version, cipher)
**Labels:** `database`, `version`, `cipher`
**Value:** Always `1` when TLS connection is active

**Use case:** Monitor TLS protocol versions and cipher suites
```promql
# Show active TLS versions
dbpulse_tls_info{version=~"TLS.*"}

# Alert on old TLS versions
dbpulse_tls_info{version=~"TLSv1|TLSv1.1"} > 0
```

#### `dbpulse_tls_handshake_duration_seconds`
**Type:** Histogram
**Description:** TLS handshake duration in seconds
**Labels:** `database`

**Use case:** Monitor TLS handshake performance
```promql
# Average TLS handshake duration
rate(dbpulse_tls_handshake_duration_seconds_sum[5m]) / rate(dbpulse_tls_handshake_duration_seconds_count[5m])
```

#### `dbpulse_tls_connection_errors_total`
**Type:** Counter
**Description:** Total TLS connection errors
**Labels:** `database`, `error_type`

**Use case:** Detect TLS certificate or configuration issues
```promql
# Rate of TLS errors
rate(dbpulse_tls_connection_errors_total[5m])

# Alert on TLS errors
increase(dbpulse_tls_connection_errors_total[5m]) > 0
```

### Example Prometheus Alerts

```yaml
groups:
  - name: dbpulse
    rules:
      # Database unavailable
      - alert: DatabaseDown
        expr: dbpulse_pulse == 0
        for: 2m
        labels:
          severity: critical
        annotations:
          summary: "Database is unavailable"
          description: "Database has been unavailable for 2 minutes"

      # High latency
      - alert: DatabaseHighLatency
        expr: sum(rate(dbpulse_runtime_sum[5m])) / sum(rate(dbpulse_runtime_count[5m])) > 1
        for: 5m
        labels:
          severity: warning
        annotations:
          summary: "Database latency is high"
          description: "Database operations are taking longer than 1 second"

      # TLS errors
      - alert: DatabaseTLSErrors
        expr: increase(dbpulse_tls_connection_errors_total[5m]) > 0
        labels:
          severity: warning
        annotations:
          summary: "TLS connection errors detected"
          description: "Database TLS connections are failing"
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

### Available tags

- `latest` - Latest stable release
- `x.y.z` - Specific version (e.g., `0.5.3`)
- `x.y` - Minor version (e.g., `0.5`)
- `x` - Major version (e.g., `0`)

### Multi-architecture support

Images are built for:
- `linux/amd64` - x86_64 architecture
- `linux/arm64` - ARM64 architecture (AWS Graviton, Raspberry Pi, etc.)

## rpm

To create an RPM package:

```sh
just rpm
```
> you need to have `just` installed and docker running

Then you need to copy the `dbpulse*.x86_64.rpm`:

```sh
cp target/generate-rpm/dbpulse-*-x86_64.rpm /host
```
