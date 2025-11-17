[![Build Status](https://github.com/nbari/dbpulse/actions/workflows/build.yml/badge.svg)](https://github.com/nbari/dbpulse/actions/workflows/build.yml)
[![Test Status](https://github.com/nbari/dbpulse/actions/workflows/test.yml/badge.svg)](https://github.com/nbari/dbpulse/actions/workflows/test.yml)
[![Coverage](https://codecov.io/gh/nbari/dbpulse/graph/badge.svg?token=I7X5VOMML6)](https://codecov.io/gh/nbari/dbpulse)
[![Crates.io](https://img.shields.io/crates/v/dbpulse.svg)](https://crates.io/crates/dbpulse)
[![License](https://img.shields.io/crates/l/dbpulse.svg)](https://github.com/nbari/dbpulse/blob/master/LICENSE)
[![GHCR](https://ghcr-badge.egpl.dev/nbari/dbpulse/latest_tag?trim=major&label=latest)](https://github.com/nbari/dbpulse/pkgs/container/dbpulse)

# dbpulse 🩺

A lightweight database health monitoring tool that continuously tests database availability for read and write operations. It exposes Prometheus-compatible metrics for monitoring database health, performance, and operational metrics.

## Overview

Like a paramedic checking for a pulse, `dbpulse` performs quick vital sign checks on your database. It goes beyond simple connection tests by performing real database operations (INSERT, SELECT, UPDATE, DELETE, transaction rollback) at regular intervals to verify that your database is truly alive and accepting writes, not just accepting connections.

**Quick Pulse Check:** Is the database responsive and healthy? ✅
**Vital Signs:** Latency, errors, read-only status, replication lag 📊
**Emergency Indicators:** Blocking queries, locked tables, connectivity issues 🚨

This is particularly useful for:

- **Galera Clusters** - Detecting HALT/LOCK cases where DDL statements stall the cluster or flow-control prevents COMMITS/WRITES
- **Read-Only Detection** - Identifying when databases enter read-only mode (replicas, maintenance, failover scenarios)
- **Replication Monitoring** - Tracking replication lag on replica databases
- **Lock Detection** - Identifying blocking queries that prevent other operations
- **Performance Monitoring** - Measuring query latency, connection times, and operation throughput

The tool protects itself from hanging on locked tables using configurable timeouts (5s statement timeout, 2s lock timeout), ensuring the health probe remains responsive.

## Quick Start

```sh
# PostgreSQL
dbpulse --dsn "postgres://user:password@tcp(localhost:5432)/mydb"

# MySQL/MariaDB
dbpulse --dsn "mysql://user:password@tcp(localhost:3306)/mydb"

# With custom interval and range
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/prod" \
  --interval 60 \
  --range 1000 \
  --port 9300
```

Access metrics at `http://localhost:9300/metrics`

## Usage

### Command-Line Options

```
dbpulse [OPTIONS] --dsn <DSN>
```

#### Required Options

| Option | Environment Variable | Description |
|--------|---------------------|-------------|
| `-d, --dsn <DSN>` | `DBPULSE_DSN` | Database connection string (see DSN Format below) |

#### Optional Settings

| Option | Environment Variable | Default | Description |
|--------|---------------------|---------|-------------|
| `-i, --interval <SECONDS>` | `DBPULSE_INTERVAL` | `30` | Seconds between health checks |
| `-p, --port <PORT>` | `DBPULSE_PORT` | `9300` | HTTP port for `/metrics` endpoint |
| `-l, --listen <IP>` | `DBPULSE_LISTEN` | `[::]` | IP address to bind to (supports IPv4 and IPv6) |
| `-r, --range <RANGE>` | `DBPULSE_RANGE` | `100` | Upper limit for random ID generation (prevents conflicts in multi-instance setups) |

### DSN Format

The Data Source Name (DSN) follows this format:

```
<driver>://<user>:<password>@tcp(<host>:<port>)/<database>[?param1=value1&param2=value2]
```

**Supported drivers:** `postgres`, `mysql`

#### Basic Examples

```sh
# PostgreSQL
postgres://dbuser:secret@tcp(localhost:5432)/production

# MySQL/MariaDB
mysql://root:password@tcp(db.example.com:3306)/myapp

# With custom port
postgres://admin:pass@tcp(10.0.1.50:5433)/metrics_db

# Unix socket (PostgreSQL)
postgres://user:pass@unix(/var/run/postgresql)/mydb
```

#### TLS/SSL Parameters

Configure TLS directly in the DSN query string:

| Parameter | Values | Description |
|-----------|--------|-------------|
| `sslmode` | `disable`, `require`, `verify-ca`, `verify-full` | TLS mode (default: `disable`) |
| `sslrootcert` or `sslca` | `/path/to/ca.crt` | CA certificate for server verification |
| `sslcert` | `/path/to/client.crt` | Client certificate (mutual TLS) |
| `sslkey` | `/path/to/client.key` | Client private key (mutual TLS) |

**TLS Mode Details:**
- `disable` - No encryption (plaintext)
- `require` - Encrypted connection, no certificate verification
- `verify-ca` - Verify server certificate against CA
- `verify-full` - Verify certificate and hostname match

#### TLS Examples

```sh
# PostgreSQL with TLS required
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/prod?sslmode=require"

# PostgreSQL with full certificate verification
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/prod?sslmode=verify-full&sslrootcert=/etc/ssl/certs/ca.crt"

# MySQL with CA verification
dbpulse --dsn "mysql://user:pass@tcp(db.example.com:3306)/prod?sslmode=verify-ca&sslca=/etc/ssl/ca.crt"

# Mutual TLS (client certificates)
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/prod?sslmode=verify-full&sslrootcert=/etc/ssl/ca.crt&sslcert=/etc/ssl/client.crt&sslkey=/etc/ssl/client.key"
```

### Environment Variables

All options can be set via environment variables:

```sh
export DBPULSE_DSN="postgres://user:pass@tcp(localhost:5432)/mydb"
export DBPULSE_INTERVAL=60
export DBPULSE_PORT=9300
export DBPULSE_RANGE=1000

dbpulse  # Uses environment variables
```

### Complete Examples

**Production PostgreSQL with TLS:**
```sh
dbpulse \
  --dsn "postgres://monitor:secret@tcp(prod-db.example.com:5432)/app?sslmode=verify-full&sslrootcert=/etc/ssl/certs/ca-bundle.crt" \
  --interval 30 \
  --port 9300 \
  --range 1000
```

**MySQL Cluster Monitoring:**
```sh
dbpulse \
  --dsn "mysql://healthcheck:pass@tcp(galera-lb.internal:3306)/monitoring" \
  --interval 15 \
  --listen "0.0.0.0" \
  --port 8080
```

**Development Setup:**
```sh
dbpulse --dsn "postgres://postgres:postgres@tcp(localhost:5432)/test" -i 10 -r 50
```

## How It Works

dbpulse performs database health checks in a simple, repeating cycle:

### 1. Configuration from DSN

All TLS/SSL settings come from the DSN query parameters (no separate flags):

```bash
# TLS configuration is in the DSN string
--dsn "postgres://user:pass@host:5432/db?sslmode=verify-full&sslrootcert=/etc/ssl/ca.crt"
```

The DSN parser extracts `sslmode`, `sslrootcert`, `sslcert`, and `sslkey` parameters into a `TlsConfig` struct used for both database and certificate connections.

### 2. Health Check Cycle

Every interval (default: 30 seconds), dbpulse makes **two connections**:

**Connection #1 - Database Operations (SQLx):**
- Connects with proper TLS verification based on `sslmode`
- Executes write test (INSERT/UPDATE with unique UUID)
- Verifies read operation (SELECT to confirm data)
- Collects metrics (table size, replication lag, blocking queries)
- Queries TLS info from database (`pg_stat_ssl` or `SHOW STATUS LIKE 'Ssl%'`)

**Connection #2 - Certificate Inspection (Probe):**
- Opens separate TLS connection to database server
- Performs STARTTLS negotiation (protocol-specific)
- Extracts certificate metadata (subject, issuer, expiry date)
- Closes immediately (no database queries)

Both connections use the same TLS configuration from the DSN. The probe connection uses a `NoVerifier` to inspect certificates without validation (actual security happens in Connection #1).

**Why two connections?** SQLx doesn't expose peer certificates from its internal TLS stream, so certificate metadata must be extracted separately.

### 3. Metrics Export

Results are merged and exposed as Prometheus metrics on `/metrics`:
- Health status, latency, error rates
- TLS version, cipher suite (from Connection #1)
- Certificate subject, issuer, expiry days (from Connection #2)

**Performance:** Each check takes ~100-150ms total. Certificate caching can reduce this by 98% (see `src/tls/cache.rs`).

---

## What It Monitors

### Health Check Operations (The Pulse Check 🩺)

Every interval, dbpulse performs a quick vital signs check:

1. **Connection Test** ⚡ - Establishes database connection with timeouts
2. **Version Check** 🔍 - Retrieves database version
3. **Read-Only Detection** 🔒 - Checks if database accepts writes
4. **Write Operation** ✍️ - `INSERT` or `UPDATE` with unique ID and UUID
5. **Read Verification** ✅ - `SELECT` to verify written data matches
6. **Transaction Test** 🔄 - Tests rollback capability
7. **Cleanup** 🧹 - Deletes old records (keeps table size bounded)

**Timeout Protection:**
- PostgreSQL: 5s statement timeout, 2s lock timeout
- MySQL/MariaDB: 5s max execution time, 2s lock wait timeout

These timeouts prevent the health probe from hanging on locked tables.

### Operational Metrics (Best-effort)

In addition to health checks, dbpulse collects:

- **Replication Lag** - For replica databases only (PostgreSQL: `pg_last_xact_replay_timestamp()`, MySQL: `SHOW REPLICA STATUS`)
- **Blocking Queries** - Count of queries currently blocking others
- **Database Size** - Total database size in bytes
- **Table Size** - Monitoring table size and row count
- **Connection Duration** - How long connections are held open
- **TLS Handshake Time** - When TLS is enabled

All operational metrics use `if let Ok(...)` pattern - they never fail the health check.

## Metrics

dbpulse exposes comprehensive Prometheus-compatible metrics on the `/metrics` endpoint.

### Core Health Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_pulse` | Gauge | Binary health status (1=healthy, 0=unhealthy) |
| `dbpulse_runtime` | Histogram | Total health check duration (seconds) |
| `dbpulse_iterations_total` | Counter | Total checks by status (success/error) |
| `dbpulse_last_success_timestamp_seconds` | Gauge | Unix timestamp of last successful check |
| `dbpulse_database_readonly` | Gauge | Read-only mode indicator (1=read-only, 0=read-write) |

### Performance Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_operation_duration_seconds` | Histogram | Duration by operation (connect, insert, select, etc.) |
| `dbpulse_connection_duration_seconds` | Histogram | How long connections are held open |
| `dbpulse_connections_active` | Gauge | Currently active database connections |

### Database Operations

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_rows_affected_total` | Counter | Total rows affected by operation type (insert, delete) |
| `dbpulse_table_size_bytes` | Gauge | Monitoring table size in bytes |
| `dbpulse_table_rows` | Gauge | Approximate row count in monitoring table |
| `dbpulse_database_size_bytes` | Gauge | Total database size in bytes |

### Replication & Blocking

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_replication_lag_seconds` | Histogram | Replication lag for replica databases |
| `dbpulse_blocking_queries` | Gauge | Number of queries currently blocking others |

### Error Tracking

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_errors_total` | Counter | Total errors by type (authentication, timeout, connection, transaction, query) |
| `dbpulse_panics_recovered_total` | Counter | Total panics recovered from |

### TLS/SSL Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_tls_handshake_duration_seconds` | Histogram | TLS handshake duration |
| `dbpulse_tls_connection_errors_total` | Counter | TLS-specific connection errors |
| `dbpulse_tls_info` | Gauge | TLS version and cipher suite (labels: version, cipher) |
| `dbpulse_tls_cert_expiry_days` | Gauge | Days until TLS certificate expiration (negative if expired) |

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

# TLS certificate expiry (days remaining)
dbpulse_tls_cert_expiry_days

# Certificates expiring within 30 days
dbpulse_tls_cert_expiry_days < 30 and dbpulse_tls_cert_expiry_days > 0
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

- alert: TLSCertificateExpiringSoon
  expr: dbpulse_tls_cert_expiry_days < 30 and dbpulse_tls_cert_expiry_days > 0
  for: 1h
  labels:
    severity: warning
  annotations:
    summary: "TLS certificate expires in {{ $value }} days"
    description: "Database {{ $labels.database }} TLS certificate will expire soon"

- alert: TLSCertificateExpired
  expr: dbpulse_tls_cert_expiry_days < 0
  for: 5m
  labels:
    severity: critical
  annotations:
    summary: "TLS certificate has expired"
    description: "Database {{ $labels.database }} TLS certificate expired {{ $value | abs }} days ago"
```


## Database Permissions

The monitoring user needs these permissions:

**PostgreSQL:**
```sql
-- Create monitoring user
CREATE USER dbpulse_monitor WITH PASSWORD 'secret';

-- Grant database access
GRANT CONNECT ON DATABASE mydb TO dbpulse_monitor;
GRANT CREATE ON DATABASE mydb TO dbpulse_monitor;  -- Optional: allows auto-creation

-- Grant schema access
GRANT USAGE ON SCHEMA public TO dbpulse_monitor;
GRANT CREATE ON SCHEMA public TO dbpulse_monitor;

-- Allow table creation and operations
GRANT CREATE ON SCHEMA public TO dbpulse_monitor;
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES TO dbpulse_monitor;
```

**MySQL/MariaDB:**
```sql
-- Create monitoring user
CREATE USER 'dbpulse_monitor'@'%' IDENTIFIED BY 'secret';

-- Grant necessary permissions
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP ON mydb.* TO 'dbpulse_monitor'@'%';
GRANT REPLICATION CLIENT ON *.* TO 'dbpulse_monitor'@'%';  -- For replication lag monitoring
GRANT PROCESS ON *.* TO 'dbpulse_monitor'@'%';  -- For blocking query detection

FLUSH PRIVILEGES;
```

**Minimal Permissions (read-only monitoring):**
If the `dbpulse_rw` table already exists, only these are needed:
```sql
-- PostgreSQL
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE dbpulse_rw TO dbpulse_monitor;

-- MySQL
GRANT SELECT, INSERT, UPDATE, DELETE ON mydb.dbpulse_rw TO 'dbpulse_monitor'@'%';
```

**Note:** dbpulse will attempt to create the database if it doesn't exist (requires appropriate permissions).

## Monitoring Table

dbpulse creates and manages a table named `dbpulse_rw` (or custom name if using multiple instances) with this schema:

**PostgreSQL:**
```sql
CREATE TABLE IF NOT EXISTS dbpulse_rw (
    id INT NOT NULL,
    t1 BIGINT NOT NULL,
    t2 TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    uuid UUID,
    PRIMARY KEY(id)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_uuid ON dbpulse_rw(uuid);
CREATE INDEX IF NOT EXISTS idx_t2 ON dbpulse_rw(t2);
```

**MySQL/MariaDB:**
```sql
CREATE TABLE IF NOT EXISTS dbpulse_rw (
    id INT NOT NULL,
    t1 BIGINT NOT NULL,
    t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    uuid CHAR(36) CHARACTER SET ascii,
    PRIMARY KEY(id),
    UNIQUE KEY(uuid),
    INDEX idx_t2 (t2)
) ENGINE=InnoDB;
```

### Table Cleanup

The table is automatically maintained:
- **Hourly cleanup**: Records older than 1 hour are deleted (LIMIT 10000 per check)
- **Periodic drop**: Table is completely dropped and recreated every hour (when row count < 100k and at minute 0)
- **Bounded growth**: Table size remains small even with frequent checks

### Custom Table Names

Use different table names for multiple monitoring instances:
```sh
# Instance 1
dbpulse --dsn "postgres://user:pass@tcp(db:5432)/prod" --range 1000

# Instance 2 (different range = different table name)
dbpulse --dsn "postgres://user:pass@tcp(db:5432)/prod" --range 2000
```

## Deployment

### Container Image

Container images are automatically published to [GitHub Container Registry](https://github.com/nbari/dbpulse/pkgs/container/dbpulse) on each release.

**Pull the image:**
```sh
podman pull ghcr.io/nbari/dbpulse:latest
```

**Run with Docker/Podman:**
```sh
# PostgreSQL
podman run -d \
  --name dbpulse \
  -p 9300:9300 \
  -e DBPULSE_DSN="postgres://user:password@host.docker.internal:5432/mydb" \
  ghcr.io/nbari/dbpulse:latest

# MySQL/MariaDB with TLS
docker run -d \
  --name dbpulse \
  -p 9300:9300 \
  -v /etc/ssl/certs:/etc/ssl/certs:ro \
  -e DBPULSE_DSN="mysql://user:pass@tcp(db.example.com:3306)/prod?sslmode=verify-ca&sslca=/etc/ssl/certs/ca.crt" \
  -e DBPULSE_INTERVAL=60 \
  ghcr.io/nbari/dbpulse:latest
```

**Multi-architecture support:**
- `linux/amd64` - x86_64 architecture
- `linux/arm64` - ARM64 architecture (AWS Graviton, Apple Silicon, Raspberry Pi)

### Systemd Service

```ini
[Unit]
Description=Database Pulse Monitor
After=network.target

[Service]
Type=simple
User=dbpulse
Group=dbpulse
Environment="DBPULSE_DSN=postgres://monitor:secret@tcp(localhost:5432)/prod?sslmode=verify-full&sslrootcert=/etc/ssl/certs/ca.crt"
Environment="DBPULSE_INTERVAL=30"
Environment="DBPULSE_PORT=9300"
ExecStart=/usr/local/bin/dbpulse
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Save to `/etc/systemd/system/dbpulse.service`, then:
```sh
sudo systemctl daemon-reload
sudo systemctl enable dbpulse
sudo systemctl start dbpulse
sudo systemctl status dbpulse
```

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
