## 0.6.4 (2025-11-16)

### Fixed
* **MariaDB Compatibility**: Fixed query timeout configuration to support both MySQL and MariaDB
  - MySQL uses `max_execution_time` (milliseconds), MariaDB uses `max_statement_time` (seconds)
  - Code now attempts MySQL variable first, falls back to MariaDB variable if not supported
  - Ensures timeout protection works correctly on both database platforms

### Added
* **Enhanced Database Health Monitoring**
  - Query timeout protection to prevent hanging on locked tables:
    - PostgreSQL: `statement_timeout` (5s) and `lock_timeout` (2s)
    - MySQL/MariaDB: `max_execution_time` (5000ms) and `innodb_lock_wait_timeout` (2s)
  - Transaction read-only detection for PostgreSQL using `transaction_read_only` setting
  - Replication lag monitoring for replica databases:
    - PostgreSQL: Uses `pg_last_xact_replay_timestamp()` to measure replay lag
    - MySQL/MariaDB: Uses `SHOW REPLICA STATUS` to get `Seconds_Behind_Source`
  - Blocking query detection:
    - PostgreSQL: Monitors `pg_stat_activity` for queries with `wait_event_type = 'Lock'`
    - MySQL/MariaDB: Monitors `information_schema.processlist` for queries with lock states
  - Database size monitoring:
    - PostgreSQL: Uses `pg_database_size()` for total database size
    - MySQL/MariaDB: Sums `data_length + index_length` from `information_schema.TABLES`
* **New Prometheus Metrics**
  - `dbpulse_replication_lag_seconds`: Histogram tracking replication lag for replica databases
  - `dbpulse_blocking_queries`: Gauge showing current count of queries blocking others
  - `dbpulse_database_size_bytes`: Gauge tracking total database size in bytes
* **Grafana Dashboard Enhancements**
  - Added Replication Lag panel (timeseries) showing average and P99 lag
  - Added Blocking Queries panel (gauge) with thresholds (yellow: 1+, red: 5+)
  - Added Database Size panel (stat) with thresholds (yellow: 1GB+, red: 10GB+)
  - Updated dashboard layout: Connection & Data Operations section now at y: 15-48

### Improved
* Better detection of read-only databases:
  - PostgreSQL now checks both `pg_is_in_recovery()` and `transaction_read_only` setting
  - MySQL/MariaDB improved handling of both integer and string values for `@@read_only`
* Enhanced error handling with proper context messages for timeout configurations
* Optimized metrics collection with conditional queries based on database state
* All operational metrics use best-effort pattern (`if let Ok(...)`) - never fail health checks
* Graceful connection closing using `conn.close().await` instead of `drop()`:
  - Prevents "Connection reset by peer" errors in database server logs
  - Proper TCP connection termination with FIN packets
  - Cleaner shutdown sequence for both PostgreSQL and MySQL/MariaDB

### Documentation
* Comprehensive README update with complete usage documentation:
  - Detailed command-line options with environment variable alternatives
  - DSN format specification and examples (PostgreSQL, MySQL, TLS configurations)
  - Complete metrics reference organized by category (health, performance, operations, replication, errors, TLS)
  - New sections: "What It Monitors" explaining health check operations and timeout protection
  - Database permissions guide for PostgreSQL and MySQL/MariaDB
  - Monitoring table schema and automatic cleanup behavior
  - Deployment guides: Docker/Podman, Kubernetes, Systemd service
* Updated CHANGELOG with detailed feature descriptions and implementation specifics

## 0.6.3 (2025-11-16)

### Changed
* **TLS Configuration via DSN Query Parameters** - Simplified TLS setup
  - Removed CLI flags: `--tls-mode`, `--tls-ca`, `--tls-cert`, `--tls-key`
  - TLS now configured directly in DSN query string
  - PostgreSQL: `?sslmode=require&sslrootcert=/path/to/ca.crt`
  - MySQL/MariaDB: `?ssl-mode=require&ssl-ca=/path/to/ca.crt`
  - Supports both PostgreSQL-style (`sslmode`, `sslrootcert`) and MySQL-style (`ssl-mode`, `ssl-ca`) parameters
  - Works with both `tcp()` and `unix()` DSN protocols
  - More consistent with standard database connection strings

### Improved
* **Container Build Optimization** - 87% faster builds
  - Added ARM64 to build matrix (native compilation for both architectures)
  - Container build now uses pre-built binaries from build artifacts
  - Reduced container build time from 1h 34m to ~3 minutes
  - Total release workflow time: 1h 44m → ~13 minutes
  - Simplified Dockerfile from 61 lines to 22 lines
  - Binary consistency: container uses same binaries as release artifacts
* **Grafana Dashboard Updates**
  - Updated to Grafana 11.x (schema version 39)
  - Added `__inputs` and `__requires` sections for grafana.com compatibility
  - Fixed typo: `dbpuse_pulse` → `dbpulse_pulse` in metrics and tests
  - Removed version number from dashboard title
  - Updated all panel plugin versions to 11.0.0
  - Improved Overview section with stat panels for better visibility:
    - Health Status: Changed from gauge to stat panel with background color mode
    - Database Mode: Changed from gauge to stat panel with background color mode
    - Time Since Last Success: Changed from gauge to stat panel with background color mode
  - Reorganized dashboard layout for better workflow:
    - Overview (y: 0-6): Health status, success rate, database mode, uptime metrics
    - Performance (y: 6-15): Latency percentiles, operation duration breakdown
    - Connection & Data Operations (y: 15-32): Connection metrics, rows affected, table size
    - TLS/SSL Monitoring (y: 32-47): TLS handshake duration, connection errors, cipher info
    - Errors & Reliability (y: 47-64): Error rates, iterations, panics, error distribution (moved to bottom)
  - Ready for import at grafana.com

### Documentation
* Added comprehensive TLS configuration section to README
  - DSN format examples for PostgreSQL and MySQL
  - TLS parameter reference table
  - Examples for all TLS modes (disable, require, verify-ca, verify-full)
  - Mutual TLS (mTLS) configuration examples
* Updated all documentation to reflect DSN-based TLS configuration
* Improved CI/CD documentation with test tag workflow

## 0.6.0 (2025-11-14)

**MAJOR RELEASE** - Complete metrics overhaul with breaking changes

### Breaking Changes
* **Dependency Removal**: Removed `lazy_static` dependency in favor of `std::sync::LazyLock`
  - Metrics are now initialized using Rust 1.80+ standard library
  - If you were directly importing metrics from this crate, you may need to update your code
  - No breaking changes for normal CLI usage
  - Requires Rust 1.80 or later (edition 2024)

### Added
* Container images now published to GitHub Container Registry (GHCR)
* Multi-architecture container support (linux/amd64, linux/arm64)
* Automated container image publishing on release
* Comprehensive metrics documentation with Prometheus query examples
* Example Prometheus alert rules for database monitoring
* **Extensive Test Suite Improvements**:
  - Added 23 new unit tests (49 total tests, up from 26)
  - Comprehensive metrics testing (10 new tests)
  - Pulse module testing (9 new tests)
  - Actions module testing (4 new tests)
  - Code coverage improved from 27.74% to 45.08%
  - Robustness test suite (12 tests) covering:
    - Panic recovery in monitoring iterations
    - JoinHandle monitoring and failure detection
    - Graceful shutdown coordination
    - State integrity across failure boundaries
    - Stress testing with 1000+ iterations
* **Enhanced Prometheus Metrics Suite** - Complete observability overhaul with 11 new metrics:
  - **Error Classification Metrics** (`dbpulse_errors_total`):
    - Tracks errors by type: authentication, timeout, connection, transaction, query
    - Enables targeted alerting and debugging
  - **Operation Duration Breakdown** (`dbpulse_operation_duration_seconds`):
    - Per-operation timing: connect, create_table, insert, select, transaction_test, cleanup
    - Identifies performance bottlenecks at query level
  - **Connection Lifecycle Tracking**:
    - `dbpulse_connections_active` - Currently active connections
    - `dbpulse_connection_duration_seconds` - Total connection hold time
    - Detects connection leaks and pooling issues
  - **Row Tracking** (`dbpulse_rows_affected_total`):
    - Records rows affected by insert, update, delete operations
    - Validates cleanup effectiveness
  - **Iteration Counters** (`dbpulse_iterations_total`):
    - Success/error counts over time
    - Calculate success rates and failure trends
  - **Last Success Timestamp** (`dbpulse_last_success_timestamp_seconds`):
    - Unix timestamp of last successful check
    - Enables staleness detection alerts
  - **Table Size Monitoring**:
    - `dbpulse_table_size_bytes` - Approximate table size in bytes
    - `dbpulse_table_rows` - Approximate row count
    - Detects unbounded table growth
  - **Panic Recovery Counter** (`dbpulse_panics_recovered_total`):
    - Tracks panic frequency in production
    - Identifies stability issues
  - **Database Read-Only Status** (`dbpulse_database_readonly`):
    - Detects failover and replica promotion scenarios
    - 1 = read-only mode, 0 = read-write mode
  - **TLS Handshake Duration** (`dbpulse_tls_handshake_duration_seconds`):
    - Now properly recorded (previously defined but unused)
    - Measures TLS connection establishment time
* **Comprehensive Documentation**:
  - New `grafana/README.md` (643 lines) with complete metrics reference
  - PromQL query examples for all metrics
  - Alert rules for production monitoring
  - Recording rules for performance optimization
  - Best practices and troubleshooting guide
  - `COVERAGE_REPORT.md` with detailed test coverage analysis
  - `CODE_QUALITY_REPORT.md` with security audit and recommendations
* **Grafana Dashboard Rewrite**:
  - Completely redesigned dashboard with 18 panels (up from 7)
  - Organized into 5 logical sections: Overview, Performance, Errors & Reliability, Connection & Data Operations, TLS/SSL Monitoring
  - All new metrics integrated with proper visualizations
  - Clear panel descriptions and appropriate thresholds

### Improved
* **Dependency Reduction**: Replaced `lazy_static` crate with `std::sync::LazyLock`
  - Zero-dependency solution using Rust standard library (stable since 1.80)
  - Reduced compilation time and dependency tree
  - Better performance with lower initialization overhead
  - Improved IDE support and error messages
* Query optimizations to prevent database server overload:
  - Added `LIMIT 10000` to DELETE cleanup operations
  - Prevents long-running DELETE queries that could block other operations
* Safer DROP TABLE logic with row count checks:
  - Only drops tables with fewer than 100,000 rows
  - Uses `DROP TABLE IF EXISTS` for safer execution
  - Prevents disruption when tables have accumulated significant data
* Integration tests now use unique table names per test
  - Eliminates race conditions and table collisions
  - Enables safe parallel test execution
  - Better test isolation using `test_rw_with_table()` function
* Performance optimizations in core monitoring loop:
  - Metrics now register directly with custom registry (eliminates clone overhead)
  - TLS error detection optimized to reduce string allocations
  - Time calculations optimized to avoid redundant timestamp calls
  - Reduced memory allocations in error paths

### Fixed
* Database cleanup operations now complete in predictable time
* Concurrent tests no longer interfere with each other
* Monitoring loop now resilient to panics in individual iterations
* Application now properly detects and exits when monitoring task fails
* Added panic recovery to prevent silent failures

## 0.5.2
* `dbpulse` db will be created if it does not exist
* checks if db is in read-only mode

## 0.5.0
* Added `--range` option to define the upper limit of the range of the random number

## 0.4.0
* Added support for postgresql

## 0.3.0
* Added Prometheus /metrics endpoint
