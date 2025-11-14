## 0.5.4 (Unreleased)

### Added
* Container images now published to GitHub Container Registry (GHCR)
* Multi-architecture container support (linux/amd64, linux/arm64)
* Automated container image publishing on release
* Comprehensive metrics documentation with Prometheus query examples
* Example Prometheus alert rules for database monitoring
* Extensive robustness test suite (12 tests) covering:
  - Panic recovery in monitoring iterations
  - JoinHandle monitoring and failure detection
  - Graceful shutdown coordination
  - State integrity across failure boundaries
  - Stress testing with 1000+ iterations
* **Enhanced Prometheus Metrics Suite** - Complete observability overhaul:
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

### Improved
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
