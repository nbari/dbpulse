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
