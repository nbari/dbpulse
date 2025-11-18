## 0.8.0 (2025-11-17)

### Breaking Changes
* **TLS Dependency Migration**: Migrated from OpenSSL to Rustls
  - OpenSSL is no longer used or required as a build dependency
  - Rustls provides better async support and smaller binary size
  - TLS functionality remains 100% compatible (no configuration changes needed)
  - If building from source, OpenSSL development libraries are no longer required
  - Container images are now smaller and have fewer dependencies

### Added
* **TLS Certificate Caching** - Eliminates redundant certificate probe connections
  - New environment variable: `DBPULSE_TLS_CERT_CACHE_TTL` (default: 3600 seconds = 1 hour)
  - Reduces connection overhead by ~95% for typical deployments
  - Previous behavior: 2 connections per health check (1 SQLx + 1 TLS probe)
  - New behavior: 1 connection per health check (SQLx only, certificate probed once per TTL)
  - Performance impact: Reduces network overhead from 120 probes/hour to 1 probe/hour (30s interval)
  - Memory usage: Negligible (small HashMap with cached certificate metadata)
  - Configurable cache TTL for different operational requirements:
    - Default (3600s): Checks certificate once per hour
    - Quick updates (1800s): Checks every 30 minutes for production environments
    - Daily checks (86400s): Minimizes overhead for stable deployments
    - Disabled (0s): Probes every iteration (not recommended, only for testing)
  - Thread-safe implementation using `Arc<RwLock<HashMap>>` for concurrent access
  - Automatic cache expiration based on TTL (stale entries are not returned)
  - Displays cache TTL at startup for operational visibility
  - Leverages existing `CertCache` implementation from `src/tls/cache.rs`
  - Works seamlessly with both PostgreSQL and MySQL/MariaDB
* **TLS Module Refactoring** - Better code organization and maintainability
  - Refactored monolithic `src/tls.rs` (763 lines) into clean module structure:
    - `src/tls/mod.rs` - Module interface and public API
    - `src/tls/config.rs` - TLS configuration and DSN parameter parsing
    - `src/tls/metadata.rs` - Certificate metadata structures
    - `src/tls/probe.rs` - Certificate probing and extraction (505 lines)
    - `src/tls/verifier.rs` - Custom certificate verification (227 lines)
    - `src/tls/cache.rs` - Connection caching and reuse (130 lines)
  - Better separation of concerns for easier maintenance and testing
  - Improved code readability with focused modules
* **Enhanced TLS Error Observability**
  - New metric: `dbpulse_tls_cert_probe_errors_total` - Certificate probe errors by type
  - Error categorization: connection, handshake, parse, timeout
  - Better debugging capabilities for TLS certificate issues
  - Enables targeted alerting for specific TLS failure modes
* **Expanded Test Coverage** - 103 total tests (up from 92)
  - Added 11 new unit tests for TLS certificate probing:
    - Server name resolution tests (hostname, IPv4, IPv6)
    - MySQL handshake parsing tests
    - Certificate extraction edge cases
    - Error handling validation
  - All tests passing with zero warnings
* **Additional Metrics Restored from v0.7.3**
  - `dbpulse_database_version_info`: Database server version info (value is always 1)
  - `dbpulse_database_uptime_seconds`: How long the database has been up
  - `dbpulse_runtime_last_milliseconds`: Runtime of the most recent health check iteration
  - These metrics were temporarily missing in the sandbox branch but are now fully restored

### Improved
* **Code Quality** - Following Rust best practices
  - Cleaned and organized all imports following Rust style guide
  - Grouped imports by category: std, external crates, internal modules
  - Consistent import organization across all 9 source files
  - Zero clippy warnings with strict lints (pedantic + nursery)
* **TLS Implementation**
  - More idiomatic Rust code with better error handling
  - Reduced use of "dangerous" APIs for better security
  - Better async/await integration with tokio runtime
  - Improved certificate verification with proper root store handling
* **Documentation**
  - Added "How It Works" section to README explaining TLS certificate extraction
  - Describes the two-phase approach: real connection + certificate probe
  - Clear explanation of why direct certificate extraction from SQLx is complex
  - Better understanding for users and contributors
* **Dependencies**
  - Updated `webpki-roots` from 0.26 to 1.0 (latest stable version)
  - Better WebPKI root certificate handling
  - Improved compatibility and security

### Technical Details
* **Rust Edition**: Uses Rust 2024 edition for latest language features
  - Requires Rust 1.82+ for edition 2024 support
  - Utilizes let chains and other modern Rust features
* **Build System**: Optimized for faster compilation
  - Rustls has fewer dependencies than OpenSSL
  - Smaller binary size (TLS implementation is pure Rust)
  - Easier to cross-compile for different platforms

### Migration Guide
* **No Configuration Changes Required**
  - DSN format remains the same
  - CLI flags unchanged
  - Metrics names unchanged
  - Docker/Kubernetes deployments work as-is
* **Building from Source**
  - No longer need OpenSSL development libraries
  - Standard `cargo build` works on all platforms
  - Easier to set up development environment

## 0.7.3 (2025-11-16)

### Added
* **TLS Certificate Expiry Monitoring** - Proactive certificate lifecycle tracking
  - New metric: `dbpulse_tls_cert_expiry_days` - Days until TLS certificate expiration (negative if expired)
  - MySQL/MariaDB: Automatically extracts certificate metadata from `SHOW STATUS LIKE 'Ssl%'`:
    - Certificate expiry date (`Ssl_server_not_after`) parsed and converted to days remaining
    - Certificate subject DN (`Ssl_server_subject`) for audit trails
    - Certificate issuer DN (`Ssl_server_issuer`) for CA tracking
  - PostgreSQL: Notes added explaining pg_stat_ssl limitations (version/cipher only)
  - Date parsing supports MySQL format: `"Dec 31 23:59:59 2025 GMT"` with flexible GMT suffix handling
  - Enables proactive alerting before certificates expire (recommended: < 30 days warning, < 7 days critical)
* **Grafana Certificate Monitoring Panels**
  - Certificate Expiry gauge (6x8 grid): Visual indicator with color thresholds
    - Green: 60+ days (healthy)
    - Yellow: 30-60 days (plan renewal)
    - Orange: 7-30 days (warning)
    - Red: 0-7 days (critical - renew immediately)
  - Certificate Expiry Timeline (12x8 grid): Time series tracking expiry countdown over time
    - Shows trend line with 30-day threshold marker
    - Legend displays mean, min, and last values
    - Helps identify when certificates were renewed
* **Success Rate Monitoring Panel**
  - New gauge panel (6x6 grid) showing overall health check success rate over 5 minutes
  - Color thresholds: Red (0-95%), Yellow (95-99%), Green (99-100%)
  - Query: `sum(rate(dbpulse_iterations_total{status="success"}[5m])) / sum(rate(dbpulse_iterations_total[5m])) * 100`
  - Perfect for SLO tracking and at-a-glance health assessment

### Improved
* **Grafana Dashboard Visualization** - Cleaner, more professional appearance
  - Removed fill opacity from all 12 time series panels (changed from `fillOpacity: 10` to `0`)
  - Panels now display as clean lines without colored areas for better readability
  - Pulse & Runtime panel enhancements:
    - Dual Y-axis configuration: Left axis (0-1) for pulse status, Right axis (auto-scaled ms) for runtime
    - Added `axisColorMode: "series"` to color-code axes matching their data series
    - Left axis shows only 0 and 1 tick marks (`decimals: 0`) for binary pulse visualization
    - Removed min/max constraints from runtime series for proper auto-scaling
    - Width adjusted from 24 to 18 units to accommodate Success Rate gauge
* **Test Suite Expansion** - 100 unit tests (up from 86)
  - Certificate expiry date parsing tests (7 tests):
    - Valid future dates (90, 60, 365 days)
    - Expired certificates (negative days)
    - Edge cases (today, tomorrow, various formats)
    - Invalid format handling
    - Real-world MySQL date format examples
  - TLS metadata tests (5 tests):
    - Full certificate info validation
    - Expiry warning thresholds (90, 30, 7, 1, 0, -1, -30 days)
    - MySQL DN format parsing
    - Partial metadata scenarios
  - Metrics integration tests (3 tests):
    - Single database tracking
    - Multiple databases simultaneously
    - Metric updates over time (simulating certificate aging and renewal)
  - All tests use modern Rust range syntax (clippy approved)

### Documentation
* Certificate expiry tracking best practices:
  - MySQL/MariaDB: Full certificate metadata available through SQL queries
  - PostgreSQL: Certificate metadata requires external file monitoring (pg_stat_ssl limitation)
  - Recommended alert thresholds: 30 days (warning), 7 days (critical), 0 days (expired)
* Panel descriptions added for all new Grafana panels with usage guidance

## 0.7.2 (2025-11-16)

### Added
* **Version & Uptime Metrics**
  - New gauges: `dbpulse_database_version_info`, `dbpulse_database_uptime_seconds`
  - PostgreSQL collector reads `pg_postmaster_start_time()`, MySQL/MariaDB uses `SHOW GLOBAL STATUS LIKE 'Uptime'`
  - Pulse JSON log now includes `uptime_seconds` for CLI/metrics parity
* **Runtime Metrics**
  - Added `dbpulse_runtime_last_milliseconds` to capture the latest iteration runtime per database
  - Grafana “Pulse & Runtime” panel overlays pulse state (0/1) with the runtime trace on a secondary axis
* **Grafana Refresh**
  - Cleaned dashboard export (inputs/requires/templating) so imports prompt for the datasource
  - Overview rows now include database version, uptime, blocking queries, error rate, and pulse view

### Improved
* Always refresh `dbpulse_table_rows` for both PostgreSQL and MySQL so row-count panels never go stale; Grafana panel now sums by `(database, table)`
* All Postgres/MariaDB integration tests (plain + TLS) validate non-empty versions and non-negative uptime via a shared helper
* README & Grafana docs list the new metrics with PromQL examples

## 0.7.0 (2025-11-16)

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
