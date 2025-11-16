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

### Production Safety Design

`dbpulse` is designed from the ground up to be safe for production use. It performs **minimal, controlled operations** that have negligible impact on database performance.

### The Monitoring Table

Creates a single lightweight table for health checks:

**PostgreSQL:**
```sql
CREATE TABLE IF NOT EXISTS dbpulse_rw (
    id INTEGER PRIMARY KEY,
    uuid UUID NOT NULL,
    ts TIMESTAMP NOT NULL DEFAULT NOW()
)
```

**MySQL/MariaDB:**
```sql
CREATE TABLE IF NOT EXISTS dbpulse_rw (
    id INTEGER PRIMARY KEY,
    uuid VARCHAR(36) NOT NULL,
    ts TIMESTAMP DEFAULT CURRENT_TIMESTAMP
) ENGINE=InnoDB
```

**Characteristics:**
- **Small footprint**: 3 columns, typically < 1000 rows
- **Primary key**: Integer ID for fast lookups and updates
- **Indexed**: Primary key ensures O(1) operations
- **Automatic cleanup**: Old records deleted to prevent unbounded growth

### Query Operations (Per Health Check Cycle)

#### 1. Connection & Version Check
```sql
-- PostgreSQL
SELECT version();
SELECT pg_is_in_recovery();

-- MySQL/MariaDB
SELECT VERSION();
SELECT @@read_only;
```
**Impact:** Read-only, metadata query. Zero table locks, instant response.

#### 2. Timeout Protection Setup
```sql
-- PostgreSQL
SET LOCAL statement_timeout = 5000;  -- 5 seconds
SET LOCAL lock_timeout = 2000;       -- 2 seconds

-- MySQL/MariaDB
SET max_execution_time = 5000;       -- 5 seconds (milliseconds)
SET innodb_lock_wait_timeout = 2;    -- 2 seconds
```
**Safety:** Prevents health checks from hanging on locked tables or long-running queries.

#### 3. Write Operation (INSERT or UPDATE)
```sql
-- Try INSERT first (new ID)
INSERT INTO dbpulse_rw (id, uuid, ts)
VALUES ($1, $2, NOW());

-- If ID exists, UPDATE instead
UPDATE dbpulse_rw
SET uuid = $1, ts = NOW()
WHERE id = $2;
```
**Impact:**
- Single row operation (1 write per check)
- Uses primary key (indexed, O(1) lookup)
- Minimal WAL/binlog impact (~50 bytes per operation)
- No table scans, no full table locks

#### 4. Read Verification
```sql
SELECT uuid FROM dbpulse_rw WHERE id = $1;
```
**Impact:**
- Primary key lookup (O(1), uses index)
- Zero table locks
- Instant response (<1ms typically)

#### 5. Transaction Rollback Test
```sql
BEGIN;
UPDATE dbpulse_rw SET uuid = $1 WHERE id = $2;
ROLLBACK;
```
**Impact:**
- Tests transaction capability
- Changes rolled back (zero persistent impact)
- Validates MVCC/transaction isolation

#### 6. Cleanup (Periodic)
```sql
-- PostgreSQL
DELETE FROM dbpulse_rw
WHERE ts < NOW() - INTERVAL '24 hours'
LIMIT 10000;

-- MySQL/MariaDB
DELETE FROM dbpulse_rw
WHERE ts < DATE_SUB(NOW(), INTERVAL 24 HOUR)
LIMIT 10000;
```
**Safety:**
- Runs only when table has data
- `LIMIT 10000` prevents long-running DELETEs
- Uses timestamp index for efficient cleanup
- Keeps table size bounded (<1000 rows typically)

#### 7. Table Drop Protection
```sql
-- Only drops if row count < 100,000
DROP TABLE IF EXISTS dbpulse_rw;
```
**Safety:** Prevents accidental data loss if table accumulated significant data.

### Operational Metrics (Best-Effort Queries)

These queries collect additional metrics but **never fail the health check** if they error:

```sql
-- Replication Lag (PostgreSQL)
SELECT EXTRACT(EPOCH FROM (NOW() - pg_last_xact_replay_timestamp()));

-- Replication Lag (MySQL)
SHOW REPLICA STATUS;

-- Blocking Queries (PostgreSQL)
SELECT COUNT(*) FROM pg_stat_activity WHERE wait_event_type = 'Lock';

-- Blocking Queries (MySQL)
SELECT COUNT(*) FROM information_schema.processlist
WHERE state LIKE '%lock%';

-- Database Size (PostgreSQL)
SELECT pg_database_size(current_database());

-- Database Size (MySQL)
SELECT SUM(data_length + index_length)
FROM information_schema.TABLES
WHERE table_schema = DATABASE();

-- Table Statistics
SELECT pg_relation_size('dbpulse_rw');  -- PostgreSQL
SELECT data_length FROM information_schema.TABLES
WHERE table_name = 'dbpulse_rw';        -- MySQL
```

**Pattern:** All use `if let Ok(...)` - failures are logged but don't affect pulse status.

### Why It's Safe for Production

#### ✅ Minimal Resource Impact
- **1 row write** per health check (typically 30s intervals)
- **2-3 row reads** per check (primary key lookups)
- **< 100 bytes** of data per check
- **No table scans** - all queries use primary key or indexes
- **No long-running queries** - timeouts ensure operations complete in seconds

#### ✅ No Disruption to Application Traffic
- **Separate table** - isolated from application data
- **No locks on application tables** - only touches `dbpulse_rw`
- **Non-blocking operations** - primary key operations don't block readers
- **Short transaction duration** - writes complete in milliseconds

#### ✅ Bounded Resource Usage
- **Table size limited** - automatic cleanup keeps < 1000 rows
- **DELETE limits** - max 10,000 rows per cleanup prevents long locks
- **Connection pooling** - single connection per check, properly closed
- **Memory footprint** - tiny table, minimal index overhead

#### ✅ Protection Against Failures
- **Timeout protection** - never hangs on locked tables
- **Graceful degradation** - optional metrics don't fail health checks
- **Error isolation** - panic recovery prevents monitoring loop crashes
- **Connection cleanup** - proper FIN packets, no "connection reset" errors

#### ✅ Production Validation
- **100 unit tests** covering edge cases and failure modes
- **Integration tests** with real PostgreSQL and MariaDB containers
- **TLS tests** validating secure connections
- **Robustness tests** for panic recovery and concurrent operations

### Resource Estimates (30-second interval)

| Resource | Per Check | Per Hour | Per Day |
|----------|-----------|----------|---------|
| **Writes** | 1 row | 120 rows | 2,880 rows |
| **Reads** | 2-3 rows | 240-360 rows | 5,760-8,640 rows |
| **Data Written** | ~50 bytes | ~6 KB | ~144 KB |
| **WAL/Binlog** | ~50 bytes | ~6 KB | ~144 KB |
| **Disk I/O** | < 1 KB | < 120 KB | < 3 MB |
| **CPU** | < 1ms | < 2s | < 48s |

**Comparison:** A single application query typically touches more data than an entire day of health checks.

### Compatibility

- **PostgreSQL**: 9.6+ (tested with 12, 13, 14, 15, 16, 17)
- **MySQL**: 5.7+, 8.0+
- **MariaDB**: 10.x, 11.x
- **Galera Cluster**: Fully compatible, detects flow-control and HALT states
- **Cloud Databases**: AWS RDS, Aurora, Azure Database, Google Cloud SQL
- **Managed Services**: Aiven, DigitalOcean, Heroku Postgres

### Interval Scheduling Behavior

**How the interval works:**

Each health check cycle follows this pattern:

```rust
1. Start health check (record start time)
2. Perform all operations (connect, write, read, cleanup, etc.)
3. Complete health check (record end time)
4. Calculate: remaining_time = interval - actual_runtime
5. If remaining_time > 0: Sleep for remaining_time
6. If remaining_time <= 0: Start next check immediately (no sleep)
```

**Important characteristics:**

- ✅ **Operations never overlap** - Each check completes before the next starts
- ✅ **Operations never queue** - Only one check runs at a time
- ⚠️ **No breaks if operations are slow** - If runtime > interval, next check starts immediately

**Examples with different intervals:**

| Interval | Health Check Runtime | Behavior |
|----------|---------------------|----------|
| 30s | 0.5s | ✅ Sleeps 29.5s, total cycle = 30s |
| 30s | 5s | ✅ Sleeps 25s, total cycle = 30s |
| 30s | 35s | ⚠️ No sleep, next check starts immediately |
| 1s | 0.1s | ✅ Sleeps 0.9s, total cycle = 1s |
| 1s | 0.5s | ✅ Sleeps 0.5s, total cycle = 1s |
| 1s | 1.2s | ⚠️ No sleep, continuous checks |
| 1s | 2s | ⚠️ No sleep, back-to-back checks |

**⚠️ Warning: Aggressive Intervals**

Setting `--interval 1` (or any very low value) can cause issues:

**Scenario: Health check takes 2 seconds, interval set to 1 second**
```
00:00.0 - Start check #1
00:02.0 - Complete check #1 (took 2s)
00:02.0 - Start check #2 immediately (no sleep, 2s > 1s)
00:04.0 - Complete check #2
00:04.0 - Start check #3 immediately
...
```

**Result:** Continuous database operations with zero breaks between checks.

**Potential problems:**
- 🔴 **Database stress** - Constant connections, writes, and reads
- 🔴 **Connection pool exhaustion** - Rapid connection churn
- 🔴 **Metrics flooding** - Prometheus scrapes overwhelmed with data points
- 🔴 **False positives** - Timeouts due to self-induced load, not actual issues
- 🔴 **Resource waste** - CPU, network, and I/O constantly busy

**Recommended interval values:**

| Use Case | Recommended Interval | Reason |
|----------|---------------------|---------|
| **Production** | 30-60s | Balanced monitoring with minimal overhead |
| **Critical systems** | 10-15s | More frequent checks without stress |
| **Development/Testing** | 5-10s | Quick feedback during debugging |
| **High-latency networks** | 60-120s | Account for network delays |
| **Avoid** | < 5s | Risk of continuous hammering if checks are slow |

**Best Practice Formula:**
```
Recommended Interval = (Expected Health Check Duration × 3) + Safety Margin

Examples:
- Health check typically takes 0.5s → Use 5-10s interval
- Health check typically takes 2s → Use 10-30s interval
- Health check typically takes 5s → Use 30-60s interval
```

**Monitoring health check performance:**

Use the `dbpulse_runtime_last_milliseconds` metric to see how long checks actually take:

```promql
# View health check duration
dbpulse_runtime_last_milliseconds

# Alert if health checks take too long for your interval
dbpulse_runtime_last_milliseconds / 1000 > (your_interval * 0.8)
```

**Recovery from panics:**

If a health check panics (unexpected error), dbpulse:
1. Recovers from the panic (doesn't crash)
2. Sets pulse to 0 (unhealthy)
3. Increments `dbpulse_panics_recovered_total`
4. **Always sleeps for the full interval** before retrying (even if panic was quick)

This prevents panic loops from hammering the database.

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
| `dbpulse_database_version_info` | Gauge | Value `1` with `version` label describing DB server build |
| `dbpulse_database_uptime_seconds` | Gauge | How long the database has been up (seconds) |

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

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths=/etc/ssl

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
