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
dbpulse --dsn "postgres://user:password@tcp(localhost:5432)/dbpulse"

# MySQL/MariaDB
dbpulse --dsn "mysql://user:password@tcp(localhost:3306)/dbpulse"

# With custom interval and range
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/dbpulse" \
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
| `-i, --interval <SECONDS>` | `DBPULSE_INTERVAL` | `30` | Seconds between health checks (minimum `1`) |
| `-p, --port <PORT>` | `DBPULSE_PORT` | `9300` | HTTP port for `/metrics` endpoint |
| `-l, --listen <IP>` | `DBPULSE_LISTEN` | `[::]` | IP address to bind to (supports IPv4 and IPv6) |
| `-r, --range <RANGE>` | `DBPULSE_RANGE` | `100` | Upper limit for random ID generation, minimum `1` (separates row IDs in multi-instance setups) |
| N/A | `DBPULSE_TLS_CERT_CACHE_TTL` | `3600` | TLS certificate cache TTL in seconds (0 to disable caching) |

### DSN Format

The Data Source Name (DSN) follows this format:

```
<driver>://<user>:<password>@tcp(<host>:<port>)/<database>[?param1=value1&param2=value2]
```

**Supported drivers:** `postgres`, `mysql`

**PostgreSQL compatibility:** PostgreSQL 14 or newer is supported. Automated
integration tests run against PostgreSQL 18.

#### Basic Examples

```sh
# PostgreSQL
postgres://dbuser:secret@tcp(localhost:5432)/dbpulse

# MySQL/MariaDB
mysql://root:password@tcp(db.example.com:3306)/dbpulse

# With custom port
postgres://admin:pass@tcp(10.0.1.50:5433)/dbpulse

# Unix socket (PostgreSQL)
postgres://user:pass@unix(/var/run/postgresql)/dbpulse
```

#### TLS/SSL Parameters

Configure TLS directly in the DSN query string:

| Parameter | Values | Description |
|-----------|--------|-------------|
| `sslmode` or `ssl-mode` | `disable`, `require`, `verify-ca`, `verify-full` | TLS mode (default: `disable`) |
| `sslrootcert`, `sslca` or `ssl-ca` | `/path/to/ca.crt` | CA certificate for server verification |
| `sslcert` or `ssl-cert` | `/path/to/client.crt` | Client certificate (mutual TLS) |
| `sslkey` or `ssl-key` | `/path/to/client.key` | Client private key (mutual TLS) |

**TLS Mode Details:**
- `disable` - No encryption (plaintext)
- `require` - Encrypted connection, no certificate verification
- `verify-ca` - Verify server certificate against CA
- `verify-full` - Verify certificate and hostname match

#### TLS Examples

```sh
# PostgreSQL with TLS required
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/dbpulse?sslmode=require"

# PostgreSQL with full certificate verification
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/dbpulse?sslmode=verify-full&sslrootcert=/etc/ssl/certs/ca.crt"

# MySQL with CA verification
dbpulse --dsn "mysql://user:pass@tcp(db.example.com:3306)/dbpulse?sslmode=verify-ca&sslca=/etc/ssl/ca.crt"

# Mutual TLS (client certificates)
dbpulse --dsn "postgres://user:pass@tcp(db.example.com:5432)/dbpulse?sslmode=verify-full&sslrootcert=/etc/ssl/ca.crt&sslcert=/etc/ssl/client.crt&sslkey=/etc/ssl/client.key"
```

### Environment Variables

All options can be set via environment variables:

```sh
export DBPULSE_DSN="postgres://user:pass@tcp(localhost:5432)/dbpulse"
export DBPULSE_INTERVAL=60
export DBPULSE_PORT=9300
export DBPULSE_RANGE=1000
export DBPULSE_TLS_CERT_CACHE_TTL=3600  # Cache TLS certificate for 1 hour (default)

dbpulse  # Uses environment variables
```

**TLS Certificate Caching Examples:**
```sh
# Production: Check certificate every 30 minutes
export DBPULSE_TLS_CERT_CACHE_TTL=1800

# Stable environments: Check once per day
export DBPULSE_TLS_CERT_CACHE_TTL=86400

# Testing: Disable cache (probe every health check)
export DBPULSE_TLS_CERT_CACHE_TTL=0

# Default: Check once per hour (if not set)
# No need to set, 3600 is automatic
```

### Complete Examples

**Production PostgreSQL with TLS:**
```sh
dbpulse \
  --dsn "postgres://monitor:secret@tcp(prod-db.example.com:5432)/dbpulse?sslmode=verify-full&sslrootcert=/etc/ssl/certs/ca-bundle.crt" \
  --interval 30 \
  --port 9300 \
  --range 1000
```

**MySQL Cluster Monitoring:**
```sh
dbpulse \
  --dsn "mysql://healthcheck:pass@tcp(galera-lb.internal:3306)/dbpulse" \
  --interval 15 \
  --listen "0.0.0.0" \
  --port 8080
```

**Development Setup:**
```sh
dbpulse --dsn "postgres://postgres:postgres@tcp(localhost:5432)/dbpulse" -i 10 -r 50
```

## How It Works

dbpulse performs database health checks in a simple, repeating cycle:

### 1. Configuration from DSN

All TLS/SSL settings come from the DSN query parameters (no separate flags):

```bash
# TLS configuration is in the DSN string
--dsn "postgres://user:pass@host:5432/dbpulse?sslmode=verify-full&sslrootcert=/etc/ssl/ca.crt"
```

The DSN parser extracts `sslmode`, `sslrootcert`, `sslcert`, and `sslkey` parameters into a `TlsConfig` struct used for both database and certificate connections.

### 2. Health Check Cycle

Every interval (default: 30 seconds), dbpulse runs one iteration. Time runs
downwards; everything between the connect and the sleep happens inside a single
deadline, and the branches show where a check ends early or does extra work.

```mermaid
sequenceDiagram
    participant L as dbpulse
    participant DB as Database

    Note over L,DB: one check, one deadline
    L->>DB: connect, session timeouts
    L->>DB: version, uptime, replication lag
    alt read-only or in recovery
        DB-->>L: pulse 0, no write attempted
    else accepting writes
        L->>DB: INSERT, then SELECT to verify
        L->>DB: transaction rollback test
        L->>DB: delete rows older than 1 hour
        L->>DB: DROP TABLE now and then, exercises DDL
    end
    L->>DB: certificate probe, cached 1 hour
    Note over L,DB: metrics, JSON line, then sleep
```

Every step runs on every check except the `DROP TABLE` and the certificate
probe, which is why each one is a single cheap statement.

Those two are rare on purpose. The probe runs once per cache TTL, one hour by
default. The `DROP TABLE` needs both the first minute of an hour *and* a
`5`-in-`range` draw, so at the defaults it fires roughly once every ten hours,
and with `--range 1000` about once every four days.

**Connection #1 - Database Operations (SQLx):**
- Connects with proper TLS verification based on `sslmode`
- Executes write test (INSERT/UPDATE with unique UUID)
- Verifies read operation (SELECT to confirm data)
- Collects metrics (table size, replication lag, blocking queries)
- Queries TLS info from database (`pg_stat_ssl` or `SHOW STATUS LIKE 'Ssl%'`)

**Connection #2 - Certificate Inspection (Probe) - CACHED:**
- Opens separate TLS connection to database server
- Performs STARTTLS negotiation (protocol-specific)
- Extracts certificate metadata (subject, issuer, expiry date)
- Closes immediately (no database queries)
- **Cached by default**: Probe runs once per hour (configurable via `DBPULSE_TLS_CERT_CACHE_TTL`)
- Cache key: `host:port` combination
- Reduces from 120 probes/hour to 1 probe/hour with default settings

Both connections use the same TLS configuration from the DSN, and the probe
applies exactly the checks the real connection applies. Under `verify-ca` and
`verify-full` the probe validates the certificate chain against the configured
CA (or the bundled WebPKI roots) before trusting the metadata it reports, so
`dbpulse_tls_cert_expiry_days` cannot be spoofed by anything in the network
path. Hostname verification follows the mode as well: only `verify-full`
requires the certificate to name the host, matching the driver, so a
`verify-ca` deployment using an internal CA still reports certificate expiry.
Under `require` there is no trust anchor to check against, so the probe
inspects the certificate without validating it. A probe that fails verification
increments `dbpulse_tls_cert_probe_errors_total` and leaves the certificate
metrics unset rather than reporting unverified values.

**Why two connections?** SQLx doesn't expose peer certificates from its internal TLS stream, so certificate metadata must be extracted separately.

### 3. Certificate Caching

**Default behavior (1 hour cache):**
- First health check: Both Connection #1 and #2 execute (~100-150ms)
- Subsequent checks (for 1 hour): Only Connection #1 executes (~50-80ms)
- After 1 hour: Cache expires, Connection #2 runs again

**Customizing cache TTL:**
```bash
# Check certificate every 30 minutes
export DBPULSE_TLS_CERT_CACHE_TTL=1800

# Check certificate once per day
export DBPULSE_TLS_CERT_CACHE_TTL=86400

# Disable caching (probe every iteration - not recommended)
export DBPULSE_TLS_CERT_CACHE_TTL=0
```

**Performance impact:**
- Default (30s interval, 1h cache): 95% reduction in TLS probe connections
- Memory overhead: ~200 bytes per cached certificate
- Thread-safe: Uses `Arc<RwLock<HashMap>>` for concurrent access

### 4. Metrics Export

Results are merged and exposed as Prometheus metrics on `/metrics`:
- Health status, latency, error rates
- TLS version, cipher suite (from Connection #1)
- Certificate expiry days (from Connection #2, cached)

---

### 5. Pulse Line on stdout

Each completed check prints one JSON line, intended for log collection:

```json
{"runtime_ms":42,"time":"2026-09-01T09:08:52Z","version":"18.4 (Debian 18.4-1.pgdg13+1)","uptime_seconds":3600}
```

`uptime_seconds`, `tls_version` and `tls_cipher` appear when the database
reports them. When the server is not accepting writes, two more fields appear:

```json
{"runtime_ms":39,...,"read_only":true,"read_only_reason":"Transaction read-only mode enabled"}
```

Both are omitted while the database is writable.

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
- Whole check: bounded by the configured `--interval`, never less than 5s
- PostgreSQL: 5s statement timeout, 2s lock timeout
- MySQL/MariaDB: 5s max execution time, 2s lock wait timeout

The server-side statement and lock timeouts prevent the probe from hanging on
locked tables. They are applied with `SET SESSION` and so only take effect once
a connection exists; the client-side deadline additionally bounds the TCP
connect and TLS handshake, so a server that accepts connections and then goes
silent is reported as a `timeout` error instead of stalling the monitoring loop.

**Backoff:** if a check overruns the interval, the loop still pauses for at
least one second before the next one. dbpulse never runs checks back to back,
so a struggling database is not hammered by its own health probe.

### Operational Metrics (Best-effort)

In addition to health checks, dbpulse collects:

- **Replication Lag** - For replica databases only (PostgreSQL: `pg_last_xact_replay_timestamp()`, MySQL: `SHOW REPLICA STATUS`)
- **Blocked Sessions** - Count of sessions currently waiting on a lock
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
| `dbpulse_runtime_last_milliseconds` | Gauge | Duration of the most recent check, in milliseconds |
| `dbpulse_iterations_total` | Counter | Total checks by status (success/error) |
| `dbpulse_last_success_timestamp_seconds` | Gauge | Unix timestamp of last successful check |
| `dbpulse_database_readonly` | Gauge | Read-only mode indicator (1=read-only, 0=read-write) |
| `dbpulse_database_host_info` | Gauge | Current backend host serving the connection (labels: `database`, `host`) |
| `dbpulse_database_version_info` | Gauge | Server version as a label (labels: `database`, `version`) |
| `dbpulse_database_uptime_seconds` | Gauge | Server uptime in seconds |

### Performance Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_operation_duration_seconds` | Histogram | Duration by operation (connect, insert, select, etc.) |
| `dbpulse_connection_duration_seconds` | Histogram | How long connections are held open |

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
| `dbpulse_replication_lag_seconds` | Gauge | Replication lag in seconds for replica databases |
| `dbpulse_blocked_sessions` | Gauge | Number of sessions currently waiting on a lock |

### Error Tracking

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_errors_total` | Counter | Total errors by type (authentication, timeout, connection, transaction, query) |
| `dbpulse_table_recreated_total` | Counter | Times the monitoring table vanished mid-check and was recreated |
| `dbpulse_table_maintenance_errors_total` | Counter | Failures of the periodic table maintenance (labels: `database`, `operation`) |
| `dbpulse_rw_row_contention_total` | Counter | Times the read/write row was overwritten by another writer |
| `dbpulse_panics_recovered_total` | Counter | Total panics recovered from |

### TLS/SSL Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `dbpulse_tls_handshake_duration_seconds` | Histogram | TLS handshake duration |
| `dbpulse_tls_connection_errors_total` | Counter | TLS-specific connection errors |
| `dbpulse_tls_info` | Gauge | TLS version and cipher suite (labels: `database`, `version`, `cipher`) |
| `dbpulse_tls_cert_expiry_days` | Gauge | Days until TLS certificate expiration (negative if expired) |
| `dbpulse_tls_cert_probe_errors_total` | Counter | Failed certificate probes by stage (labels: `database`, `error_type`) |

Every metric is created at startup, so an alert such as `dbpulse_pulse == 0`
matches a real series from the very first scrape even if the database was never
reachable. Most metrics carry a `database` label (`mysql` or `postgres`); the
process-level ones — `dbpulse_pulse`, `dbpulse_runtime`,
`dbpulse_connection_duration_seconds` and `dbpulse_panics_recovered_total` —
describe dbpulse itself and are unlabelled.

Labelled metrics only export a line once a label combination exists. The
combinations needed for alerting (`dbpulse_iterations_total`,
`dbpulse_errors_total`, `dbpulse_database_readonly`,
`dbpulse_last_success_timestamp_seconds`, `dbpulse_runtime_last_milliseconds`,
`dbpulse_table_recreated_total`, `dbpulse_rw_row_contention_total` and
`dbpulse_table_maintenance_errors_total`) are pre-created for the monitored
database. Others — such as `dbpulse_replication_lag_seconds` on a standalone
server — appear only when they become meaningful.

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

# Current backend host (use in table/stat panel)
dbpulse_database_host_info

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

The monitoring user needs specific permissions for database operations.

**PostgreSQL:**
```sql
-- Create monitoring database
CREATE DATABASE dbpulse;

-- Create monitoring user
CREATE USER dbpulse WITH PASSWORD 'secret';

-- Grant database access
GRANT CONNECT ON DATABASE dbpulse TO dbpulse;
GRANT CREATE ON DATABASE dbpulse TO dbpulse;

-- Grant schema access
\c dbpulse
GRANT USAGE ON SCHEMA public TO dbpulse;
GRANT CREATE ON SCHEMA public TO dbpulse;

-- Allow table creation and operations
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES TO dbpulse;
```

**MySQL/MariaDB:**
```sql
-- Create monitoring database
CREATE DATABASE dbpulse;

-- Create monitoring user
CREATE USER 'dbpulse'@'%' IDENTIFIED BY 'secret';

-- Grant specific permissions (recommended)
GRANT SELECT, INSERT, UPDATE, DELETE, CREATE, DROP ON dbpulse.* TO 'dbpulse'@'%';
GRANT REPLICATION CLIENT ON *.* TO 'dbpulse'@'%';  -- For replication lag monitoring
GRANT PROCESS ON *.* TO 'dbpulse'@'%';             -- For blocking query detection

FLUSH PRIVILEGES;
```

**Alternative: GRANT ALL PRIVILEGES**

While `GRANT ALL PRIVILEGES` is simpler, it has security implications:
```sql
-- MySQL/MariaDB - NOT RECOMMENDED for production
GRANT ALL PRIVILEGES ON dbpulse.* TO 'dbpulse'@'%';
FLUSH PRIVILEGES;
```

**Constraints and security concerns:**
- ⚠️ Grants excessive permissions including `ALTER`, `INDEX`, `REFERENCES`, `LOCK TABLES`, and more
- ⚠️ User can modify table structure, which dbpulse doesn't need
- ⚠️ Violates principle of least privilege
- ⚠️ May fail security audits or compliance requirements
- ✅ Use specific permissions above for production environments

**Minimal Permissions (if table exists):**
If the `dbpulse_rw` table is already created, only these are needed:
```sql
-- PostgreSQL
GRANT SELECT, INSERT, UPDATE, DELETE ON TABLE dbpulse_rw TO dbpulse;

-- MySQL
GRANT SELECT, INSERT, UPDATE, DELETE ON dbpulse.dbpulse_rw TO 'dbpulse'@'%';
```

**Connection string with default database:**
```sh
# PostgreSQL
dbpulse --dsn "postgres://dbpulse:secret@tcp(localhost:5432)/dbpulse"

# MySQL/MariaDB
dbpulse --dsn "mysql://dbpulse:secret@tcp(localhost:3306)/dbpulse"
```

## Monitoring Table

dbpulse creates and manages a table named `dbpulse_rw` with this schema:

**PostgreSQL:**
```sql
CREATE TABLE IF NOT EXISTS dbpulse_rw (
    id INT NOT NULL PRIMARY KEY,
    t1 BIGINT NOT NULL,
    t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP,
    uuid UUID NOT NULL,
    CONSTRAINT dbpulse_rw_uuid_unique UNIQUE (uuid)
);
CREATE INDEX IF NOT EXISTS idx_dbpulse_rw_t2 ON dbpulse_rw(t2);
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
- **Every check**: records whose `t2` is older than 1 hour are deleted (`LIMIT 10000` per check)
- **Bounded growth**: writes are upserts over the ID space `0..range`, so the table holds at most `range` rows, and in practice roughly one row per check within the retention window. A default deployment keeps it in the tens of kilobytes
- **Periodic drop**: at minute 0, with a 5-in-`range` chance per check and a row count below 100k, the table is dropped and recreated on the next check

The periodic drop is a health check in its own right, not just housekeeping: it
exercises DDL. A Galera cluster that stalls on DDL, or a node under flow
control, shows up here as a failed or slow check, which is one of the conditions
dbpulse exists to detect.

Note how infrequent those two conditions make it. At the defaults two checks
fall inside minute 0 and each has a 5-in-100 chance, so a drop happens roughly
once every ten hours; with `--range 1000` it is closer to once every four days,
because the chance is `5/range`. Raising `--range` to separate instances also
makes the DDL check rarer. The gate reads the wall-clock minute rather than
tracking elapsed time, so an interval that divides an hour with a non-zero
offset never sees minute 0 at all: `--interval 1800` starting at minute 15 lands
on 15, 45, 15, ... and never drops.

### Running Multiple Instances

All instances against the same database share the table `dbpulse_rw`; the name
is not configurable. `--range` bounds the **row IDs** an instance writes: each
check picks a random ID in `0..range`, so it caps how large the table grows —
it does **not** give each instance a private slice of IDs.

```sh
# Both instances use the table dbpulse_rw in the same database.
dbpulse --dsn "postgres://user:pass@tcp(db:5432)/dbpulse" --range 1000
dbpulse --dsn "postgres://user:pass@tcp(db:5432)/dbpulse" --range 2000
```

Because every range starts at zero, two instances *will* occasionally pick the
same ID. If the second overwrites the row in the window between the first
instance's write and its read-back, the first counts
`dbpulse_rw_row_contention_total{database}` and continues rather than reporting
a failure — the row it wrote is gone, but the database did nothing wrong. A
read-back that returns data *older* than what was written is still a hard
failure, since no concurrent writer can explain it.

A high contention rate means `--range` is too small for the number of
instances; a nonzero value on a single-instance deployment means something else
is writing to the table.

Because the table is shared, one instance's periodic drop can land while another
is mid-check. That instance recreates the table and retries once rather than
reporting a failure, and counts the event on
`dbpulse_table_recreated_total{database}`. A nonzero value on a single-instance
deployment means something other than dbpulse is dropping the table.

Maintenance that fails outright — the periodic row count or `DROP TABLE` being
refused, for example by a permission change — is counted on
`dbpulse_table_maintenance_errors_total{database,operation}` rather than failing
the health check.

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
  -e DBPULSE_DSN="postgres://user:password@host.docker.internal:5432/dbpulse" \
  ghcr.io/nbari/dbpulse:latest

# MySQL/MariaDB with TLS
docker run -d \
  --name dbpulse \
  -p 9300:9300 \
  -v /etc/ssl/certs:/etc/ssl/certs:ro \
  -e DBPULSE_DSN="mysql://user:pass@tcp(db.example.com:3306)/dbpulse?sslmode=verify-ca&sslca=/etc/ssl/certs/ca.crt" \
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
Environment="DBPULSE_DSN=postgres://monitor:secret@tcp(localhost:5432)/dbpulse?sslmode=verify-full&sslrootcert=/etc/ssl/certs/ca.crt"
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
