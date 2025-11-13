# DBPulse Testing Guide

## Quick Start

### Run Unit Tests
```bash
just test
```
Runs: clippy (strict) → fmt → unit tests

### Run Integration Tests
```bash
just test-integration
```
Runs: Full integration test suite against PostgreSQL and MariaDB

### Full CI Check
```bash
just ci
```
Runs: clippy → fmt → unit tests

## Unit Tests

Unit tests cover CLI parsing, configuration, and TLS mode logic:

```bash
# Run all unit tests
cargo test

# Run with coverage
just coverage
```

**Coverage:** ~40% (100% of CLI/config code, 0% of runtime code that requires databases)

## Integration Tests

Integration tests verify actual database operations, including:
- Basic connectivity
- Read/write operations
- Transaction rollback
- Concurrent connections
- TLS connectivity
- Database auto-creation
- Error handling
- Version detection

### Test Structure

```
tests/
├── common/mod.rs         # Shared test utilities
├── postgres_test.rs      # PostgreSQL integration tests (11 tests)
└── mariadb_test.rs       # MariaDB integration tests (11 tests)
```

### Run All Integration Tests

```bash
# Full suite (PostgreSQL + MariaDB)
just test-integration

# PostgreSQL only
just test-postgres-integration

# MariaDB only
just test-mariadb-integration
```

### Run Specific Test

```bash
# PostgreSQL basic connection
cargo test --test postgres_test test_postgres_basic_connection -- --ignored --nocapture

# MariaDB TLS test
cargo test --test mariadb_test test_mariadb_tls_disable -- --ignored --nocapture
```

### Integration Test Categories

**PostgreSQL Tests (11 total):**
1. `test_postgres_basic_connection` - Basic connectivity
2. `test_postgres_read_write_operations` - Repeated R/W with cleanup
3. `test_postgres_transaction_rollback` - Transaction handling
4. `test_postgres_concurrent_connections` - 10 concurrent connections
5. `test_postgres_with_different_ranges` - Test various range values
6. `test_postgres_tls_disable` - TLS disabled mode
7. `test_postgres_tls_require` - TLS required mode (needs TLS setup)
8. `test_postgres_database_creation` - Auto-create database
9. `test_postgres_invalid_credentials` - Auth error handling
10. `test_postgres_version_info` - Version string parsing

**MariaDB Tests (11 total):**
1. `test_mariadb_basic_connection` - Basic connectivity
2. `test_mariadb_read_write_operations` - Repeated R/W with cleanup
3. `test_mariadb_transaction_rollback` - Transaction handling
4. `test_mariadb_concurrent_connections` - 10 concurrent connections
5. `test_mariadb_with_different_ranges` - Test various range values
6. `test_mariadb_tls_disable` - TLS disabled mode
7. `test_mariadb_tls_require` - TLS required mode (needs TLS setup)
8. `test_mariadb_database_creation` - Auto-create database
9. `test_mariadb_invalid_credentials` - Auth error handling
10. `test_mariadb_version_info` - Version string parsing
11. `test_mariadb_read_only_detection` - Read-only mode detection

## Local Database Testing

### Start Databases

```bash
# Start PostgreSQL
just postgres

# Start MariaDB
just mariadb

# Stop all
just stop-db
```

### Manual Testing

```bash
# PostgreSQL
cargo run -- --dsn "postgres://postgres:secret@tcp(localhost:5432)/testdb" \
  --interval 5 --range 100

# MariaDB
cargo run -- --dsn "mysql://dbpulse:secret@tcp(localhost:3306)/testdb" \
  --interval 5 --range 100

# With TLS
cargo run -- --dsn "postgres://postgres:secret@tcp(localhost:5432)/testdb" \
  --interval 5 --tls-mode require
```

## Environment Variables

### Skip Tests

```bash
# Skip PostgreSQL tests
export SKIP_POSTGRES_TESTS=1

# Skip MariaDB tests
export SKIP_MARIADB_TESTS=1
```

### Configuration

All CLI options support environment variables:

```bash
export DBPULSE_DSN="postgres://postgres:secret@tcp(localhost:5432)/testdb"
export DBPULSE_INTERVAL=10
export DBPULSE_LISTEN="127.0.0.1"
export DBPULSE_PORT=9300
export DBPULSE_RANGE=500
export DBPULSE_TLS_MODE="require"
export DBPULSE_TLS_CA="/path/to/ca.crt"
export DBPULSE_TLS_CERT="/path/to/client.crt"
export DBPULSE_TLS_KEY="/path/to/client.key"
```

## TLS Testing

### Without TLS (Default)

```bash
# Tests will pass with default container setup
just test-integration
```

### With TLS

To test TLS functionality:

1. Configure database containers with TLS enabled
2. Run TLS-specific tests:

```bash
# PostgreSQL TLS tests
cargo test --test postgres_test test_postgres_tls -- --ignored --nocapture

# MariaDB TLS tests
cargo test --test mariadb_test test_mariadb_tls -- --ignored --nocapture
```

**Note:** TLS tests will skip gracefully if databases don't have TLS configured.

## Metrics

View metrics at http://localhost:9300/metrics

**Core Metrics:**
- `dbpuse_pulse` - Health status (1=ok, 0=error)
- `dbpulse_runtime` - Pulse latency in seconds

**TLS Metrics:**
- `dbpulse_tls_info{database, version, cipher}` - TLS connection info
- `dbpulse_tls_connection_errors_total{database, error_type}` - Error counter
- `dbpulse_tls_handshake_duration_seconds{database}` - Handshake timing

## Code Quality

```bash
# Strict linting
just clippy

# Format code
just fmt

# Full CI check
just ci
```

## CI/CD

```bash
# What GitHub Actions runs
just ci
```

## Troubleshooting

### Tests Are Skipped

All integration tests use `#[ignore]` attribute. You must run them explicitly:

```bash
# Wrong (tests are skipped)
cargo test

# Correct (runs ignored tests)
cargo test -- --ignored
```

Or use justfile recipes which handle this automatically:
```bash
just test-integration
```

### Database Not Ready

Increase sleep time in justfile if tests fail with connection errors:
```bash
@sleep 5  # Increase to 10 for slower systems
```

### Port Conflicts

```bash
lsof -i :5432  # PostgreSQL
lsof -i :3306  # MariaDB
lsof -i :9300  # DBPulse metrics
```

### View Container Logs

```bash
podman logs dbpulse-postgres
podman logs dbpulse-mariadb
```

### Clean State

```bash
# Stop all containers
just stop-db

# Or force remove
podman rm -f dbpulse-postgres dbpulse-mariadb
```

## Test Development

### Adding New Tests

1. Add test function to appropriate file:
   - `tests/postgres_test.rs` for PostgreSQL
   - `tests/mariadb_test.rs` for MariaDB

2. Mark with `#[ignore]` attribute:
```rust
#[tokio::test]
#[ignore = "requires running PostgreSQL container"]
async fn test_my_new_feature() {
    // test code
}
```

3. Use skip helpers for optional tests:
```rust
if skip_if_no_postgres() {
    return;
}
```

### Running During Development

```bash
# Run specific test while developing
cargo test --test postgres_test test_postgres_basic_connection -- --ignored --nocapture

# Watch mode (requires cargo-watch)
cargo watch -x 'test --test postgres_test -- --ignored --nocapture'
```
