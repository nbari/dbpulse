## 0.10.0 (2026-09-01)

### Breaking Changes
* **`dbpulse_replication_lag_seconds` is now a Gauge** (was a Histogram)
  - **Previous behavior**: a histogram with default buckets topping out at `10s`, so a replica `300s` behind was indistinguishable from one `11s` behind, and the current lag could not be read at all
  - **New behavior**: a gauge holding the current lag in seconds, so it plots as a time series and alerts as `> 30`
  - **Impact**: queries against `_bucket`, `_sum`, and `_count` no longer resolve. `grafana/dashboard.json` is updated; external dashboards and alerts must be migrated
* **Read-Only State Removed from the Version Label** - read-only is no longer signalled by appending text to the version string
  - **Previous behavior**: `HealthCheckResult.version` became e.g. `MariaDB 11.4.5 - Database is in read-only mode`, which churned the `dbpulse_database_version_info` label on every transition
  - **New behavior**: `HealthCheckResult` carries `read_only: bool` and `read_only_reason: Option<String>`; `version` holds the version only
  - **Impact**: anything parsing the version string for read-only state must use `dbpulse_database_readonly` instead
* **`dbpulse_blocking_queries` renamed to `dbpulse_blocked_sessions`** - the metric never measured what its name claimed
  - Both engines count sessions *waiting* on a lock (PostgreSQL `wait_event_type = 'Lock'`, MySQL processlist states matching `lock`), which are the victims of contention, not the queries causing it. One blocking transaction can produce many blocked sessions
  - **Impact**: dashboards and alerts must use the new name; `grafana/dashboard.json` is updated
* **`--interval 0` and `--range 0` Are Rejected** - both are now validated at parse time (minimum `1`)

### Added
* **Client-Side Check Deadline** - the whole health check is bounded by `max(interval, 5s)`
  - The `SET SESSION` statement/lock timeouts only apply once a connection exists, so a server that accepted TCP and then went silent stalled the monitoring loop indefinitely: no iteration, no error metric, no output
  - Timeouts are recorded as `dbpulse_errors_total{error_type="timeout"}`
* **Metric Pre-Registration** - every metric is registered at startup, and the label children for the monitored database are created
  - Metrics are `LazyLock`, so an untouched one was *absent* from `/metrics` rather than `0`. An alert written as `dbpulse_pulse == 0` never fired while the first check was still running or hanging, because absent series do not match a comparison
* **Minimum Backoff Between Iterations** - a check that overruns its interval is now followed by a `1s` pause
  - Previously the sleep was skipped entirely, so an overrunning check re-ran continuously (~25 iterations/second measured), hammering a database precisely when it was already struggling
* **Missing-Table Recovery** - a check whose table disappears mid-cycle recreates it and retries once
  - dbpulse drops its own table hourly to exercise DDL, and instances share one table, so a concurrent drop made a healthy database report a failed check
  - Detected by driver error code (MySQL `1146`, PostgreSQL `42P01`), never by message matching
* **New Metric** - `dbpulse_table_recreated_total{database}` counts those recoveries, so they stay visible rather than silent
* **New Metric** - `dbpulse_table_maintenance_errors_total{database,operation}` counts periodic-maintenance failures
  - The hourly row count and `DROP TABLE` are best effort and must not fail the health check, but their errors were discarded with `.ok()`. A permission change or a lock that blocked maintenance indefinitely was invisible until the table had grown large enough to cause a real outage
  - `operation` is `count` or `drop`; both are pre-created at zero
* **Read-Only State in the JSON Pulse Line** - the stdout pulse line now carries `read_only` and `read_only_reason`, replacing the reason text that was previously appended to `version`. Both are omitted when the database is writable
* **Verified Certificate Probe** - under `sslmode=verify-ca` and `verify-full` the certificate probe now validates the server certificate against the configured CA (or the bundled WebPKI roots) before trusting the metadata it reports
  - `dbpulse_tls_cert_expiry_days` can no longer be spoofed by anything in the network path
  - Hostname verification follows the mode, matching the driver: only `verify-full` requires the certificate to name the host, so a `verify-ca` deployment using an internal CA still reports certificate expiry instead of silently losing the gauge
  - `require` keeps unverified inspection, as documented; wires in the previously unused `CertCapturingVerifier`
  - The probe is bounded by its own 3s timeout, below the 5s check deadline, so a server that completes TCP and then stalls the handshake cannot consume the caller's budget
* **Stale TLS Series Are Retired** - `dbpulse_tls_info` now removes the previous version/cipher series when either changes, and `dbpulse_tls_cert_expiry_days` is cleared when a check reports no expiry
  - Previously a renegotiated cipher or a certificate that stopped being readable left the old series exported forever, so a dashboard kept showing a comfortable expiry for a certificate dbpulse could no longer see
* **Security Policy and Supply-Chain Auditing**
  - `.github/SECURITY.md` with a private reporting process and scope
  - `.github/workflows/security-audit.yml` running `cargo-audit` and `cargo-deny` on every push and daily
  - `deny.toml` and `.cargo/audit.toml` pin the license allow-list and record accepted advisories with rationale
* **aarch64 Release Artifacts** - RPM and DEB packages are now built for `aarch64` as well as `x86_64`
  - `linux-arm64` moved from `cross` to the native `ubuntu-24.04-arm` runner
  - Added an `aarch64-apple-darwin` target

### Fixed
* **A Disconnected Standby Reported Zero Replication Lag Forever** - `dbpulse_replication_lag_seconds` treated equal receive/replay LSNs as "caught up" unconditionally. A standby whose primary has died reaches that state the moment replay drains, so it reported a lag of exactly `0` indefinitely while serving increasingly stale data -- at precisely the moment the metric has to be believed. Reproduced against a live streaming pair on PostgreSQL 18. Equal LSNs now count as caught up only while a walreceiver row is present. The guard keys off row *existence* rather than `status`, because `status` is privilege-restricted: an unprivileged monitoring role sees the row with a `NULL` status, and testing `status = 'streaming'` would have reported a growing lag for every healthy standby not monitored by a superuser
* **Two More Gauges Froze at Their Last Value** - the earlier best-effort-gauge fix covered `dbpulse_blocked_sessions`, `dbpulse_table_size_bytes` and `dbpulse_database_size_bytes` but missed `dbpulse_table_rows` and `dbpulse_database_uptime_seconds`, which kept reporting the last successful reading when their query failed or the table was gone. Both now retire the series, and every such collector routes through one `set_or_retire` helper so a new call site cannot forget the rule
* **Identifier Length Allowed a Name PostgreSQL Silently Truncates** - `validate_identifier` rejected names longer than 64 characters, but PostgreSQL truncates at 63 rather than erroring, so a 64-character table or database name passed validation and then referred to a different object than the one requested. Both engines are now held to 63
* **Rollback-Test Upsert Kept a Stale `t2`** - the PostgreSQL rollback probe's `ON CONFLICT DO UPDATE` did not refresh `t2` like the main upsert does. It changes nothing today because that transaction always rolls back, but it left the two upserts able to diverge and would have reintroduced cleanup deleting actively-written rows if the path ever committed

* **An Unrecognised `sslmode` Silently Disabled TLS** - the DSN mode was parsed with `.ok().unwrap_or_default()`, and the default is `disable`
  - A typo such as `sslmode=verify-fill`, or a MySQL-style `ssl-mode=REQUIRED` that the parser did not accept, produced a plaintext connection that carried the credentials in the clear and reported nothing
  - Unparseable modes now abort startup with an error naming the supported values, and MySQL's spellings (`DISABLED`, `REQUIRED`, `VERIFY_CA`, `VERIFY_IDENTITY`) are accepted, matching the documented `ssl-mode` alias
  - Opportunistic modes (`prefer`, `preferred`, `allow`) are rejected rather than guessed at in either direction
* **PostgreSQL Replication Lag Was Phantom on an Idle Primary** - lag was `NOW() - pg_last_xact_replay_timestamp()`, which measures the age of the last replayed transaction rather than the distance behind the primary
  - On a fully synchronised standby with an idle primary the value grew without bound: measured at `20s` and then `75s` on a replica that had nothing left to apply, firing any `> 60` alert on a perfectly healthy node
  - The receive and replay LSNs are now compared first, so a caught-up standby reports exactly `0`; genuine lag is still reported in seconds, and a primary still reports no series at all
* **A Concurrent Instance Looked Like Data Corruption** - the read/write check compared the read-back row for exact equality, so a second dbpulse instance overwriting the shared row produced "Records don't match", indistinguishable from the database losing a write
  - Instances share one table and `--range` does not partition the ID space (every range starts at zero), so collisions are expected on a multi-instance deployment
  - A row carrying a timestamp at or after the one written is now counted on the new `dbpulse_rw_row_contention_total{database}` and the check continues. A row carrying an *older* timestamp is still a hard failure, since no concurrent writer can explain it
  - README previously claimed `--range` separated the row IDs each instance writes; it does not, and now says so
* **A Just-Expired Certificate Reported `0` Days** - `Duration::num_days` truncates toward zero, so everything in the 24 hours after expiry rounded up to `0`
  - The documented alerts are `< 30 and > 0` (expiring) and `< 0` (expired); `0` matches neither, so a certificate that had just lapsed was invisible to both for a full day
  - The day count is now floored, guaranteeing an expired certificate reports `<= -1`
* **Stale TLS Series Survived a Failed Check** - `record_error` cleared the host label but left `dbpulse_tls_info` and `dbpulse_tls_cert_expiry_days` untouched, so a database that had been unreachable for hours still advertised an active cipher suite and a comfortable certificate expiry
* **Best-Effort Gauges Froze at Their Last Good Value** - `dbpulse_blocked_sessions`, `dbpulse_table_size_bytes` and `dbpulse_database_size_bytes` were only written on success, so a server that stopped answering those queries kept reporting the last reading instead of going absent
* **Certificate Probe Could Overrun the Check Deadline** - the probe applied a flat 3s from whenever it happened to start, but the deadline is consumed cumulatively, so a check that had already spent 4s of a 5s allowance could be failed by optional metadata collection after its required work had succeeded
  - The probe now uses whatever remains of the deadline, capped at 3s, and is skipped entirely below 250ms
* **Dynamic SQL Identifiers Were Unvalidated** - `test_rw_with_table` is a public API whose table name is interpolated into DDL under `AssertSqlSafe`, which performs no escaping. Names are now restricted to plain identifiers. The binary only ever passes the literal `dbpulse_rw`, so nothing was exploitable in the shipped tool
* **Redundant MySQL Lock-State Pattern** - the blocked-session query OR-ed `LIKE '%lock%'` with `LIKE '%Locked%'`; the default collation is case-insensitive, so the second matched nothing the first had not
* **Replication Lag Never Reported on MariaDB** - the code read only `Seconds_Behind_Source` (MySQL 8.0.22+). MariaDB aliases `SHOW REPLICA STATUS` but kept the column name `Seconds_Behind_Master`, so the metric silently never appeared on any MariaDB replica. Both names are now tried, falling back to `SHOW SLAVE STATUS` for MariaDB before 10.5
  - Lag is now sampled on every check rather than only on the branch that also handled read-only or table recovery, so it no longer depends on unrelated state
  - A `NULL` lag (a primary, or a replica with a broken IO thread) is decoded as absent instead of failing, and removes the series rather than leaving the last value frozen in place
* **MySQL Cleanup Cutoff Computed in the Wrong Timezone** - the delete bound an RFC3339 string, which MySQL truncated with `Truncated incorrect datetime value`, discarding the UTC offset and interpreting the cutoff in the session timezone. On a server behind UTC this deleted rows far newer than the intended one-hour window. The cutoff is now `NOW() - INTERVAL 1 HOUR`, matching the PostgreSQL path
* **`--range 0` Panicked Forever** - `random_range(0..0)` panicked on every iteration; the panic handler recovered and re-entered the loop indefinitely without ever making progress
* **`just test-tls` Failed Under SELinux** - the MariaDB certificate bind mounts lacked an SELinux relabel, so the container aborted with `Failed to setup SSL`. Added `:z`
* **Podman Short-Name Resolution in `.justfile`** - image references are fully qualified, so recipes no longer fail with `short-name resolution enforced but cannot prompt without a TTY` when run non-interactively
* **Documented but Nonexistent Metric** - removed `dbpulse_connections_active` from `README.md` and `grafana/README.md`; it was never implemented, so its documented queries returned nothing and the `ConnectionLeakSuspected` alert built on it could never fire. Replaced with alerts on `dbpulse_connection_duration_seconds` and `dbpulse_table_recreated_total`
* **Certificate Subject and Issuer Documented as Exported** - the README listed them among the exported metrics, but they are only ever held in memory: no metric and no field of the JSON pulse line carries them. The claim is removed, and `dbpulse_database_version_info`, `dbpulse_database_uptime_seconds` and `dbpulse_tls_cert_probe_errors_total` (all previously undocumented) are now listed, along with the `database` label and the `ssl-mode`/`ssl-ca`/`ssl-cert`/`ssl-key` DSN aliases
* **Incorrect Multi-Instance Documentation** - the README claimed that a different `--range` produced a different table name. It does not: the table is always `dbpulse_rw`, and `--range` separates row IDs only. Instances against one database share the table, which is why the concurrent-drop recovery above is needed
* **Coverage Job Broken by a Contradictory Lint Policy** - `coverage.yml` ran `cargo clippy ... -D clippy::nursery`, overriding the `nursery = allow` policy set in `Cargo.toml`, so it enforced lints the project deliberately disables while the dedicated `test / Clippy` job stayed green. The duplicated step is removed; `test.yml` is the single lint gate and `Cargo.toml` the single lint policy
* **Health Check Cycle Diagram** - the README now opens the cycle section with a mermaid sequence diagram: one iteration on a vertical time axis, showing the single deadline everything runs inside, the read-only branch that ends a check early, the hourly `DROP TABLE` that exercises DDL, the cached certificate probe, and the interval sleep with its floor
* **Documentation Drift** - corrected the PostgreSQL schema shown in the README to match what is created, described the cleanup as running every check rather than hourly, documented the `--interval` and `--range` minimums, documented the JSON pulse line, and added the previously undocumented `dbpulse_runtime_last_milliseconds`
* **The Just-Expired-Certificate Fix Missed Two Paths** - the probe was floored, but the MySQL `Ssl_server_not_after` fallback (the only expiry source for unix-socket connections and probe failures) and `CertCapturingVerifier::extract_metadata` still used `Duration::num_days`, which truncates toward zero: a certificate expired less than 24 hours reported `0` days through them, matching neither documented alert. All three paths now share the probe's flooring helper, and the fallback is regression-tested against generated just-expired dates
* **Uppercase TLS Parameter Keys Silently Disabled TLS** - the `dsn` crate stores query parameter keys verbatim, so `?SSLMODE=verify-full` missed the case-sensitive `sslmode` lookup and fell through to `disable` -- the same plaintext fail-open as an unparseable value. TLS parameter keys are now matched case-insensitively, so a bad value under any casing aborts startup instead of downgrading
* **A Row Deleted Mid-Check Looked Like a Fault** - when a concurrent instance's cleanup removed the just-written row between the upsert and the read-back, the check failed with "Expected records". Only dbpulse deletes from the shared table, so a vanished row is now counted on `dbpulse_rw_row_contention_total` like any other concurrent interference, and the check continues
* **PostgreSQL Upserts Never Refreshed `t2`** - MySQL gets `ON UPDATE CURRENT_TIMESTAMP`; PostgreSQL has no equivalent, so a row written every interval kept its original insert timestamp and the cleanup deleted live rows an hour after creation (and, raced with another instance, produced the spurious failure above). The upsert now sets `t2 = CURRENT_TIMESTAMP`
* **`--range` Above `i32::MAX` Failed Roughly Half of All Checks** - the id column is a signed `INT` in both schemas; larger ranges produced an unwritable id about half the time (a graceful error on PostgreSQL, server error 1264 on MySQL). The CLI now rejects values above 2147483647, and the library entry points return an error for `range = 0` or an overflowing range instead of panicking or failing per-iteration
* **A Standby That Has Never Streamed Reported Lag 0** - both LSNs being NULL satisfied `IS NOT DISTINCT FROM`, so "never received anything" was treated as "fully caught up". That case now reports no series, matching a primary
* **Certificate Cache Keyed by the Wrong Port** - `get_cert_metadata_cached` keyed entries on the driver's default port instead of the DSN's, so a probe of `host:3307` was cached as `host:3306`. Harmless for the single-DSN binary; wrong for any library caller holding two DSNs on the same host
* **Unsupported Driver Only Failed After Binding** - an unknown DSN scheme was rejected on the first check, once the metrics listener was already serving. Startup now fails immediately with a message naming the driver
* **`dbpulse_table_size_bytes` on MySQL Reported 0 for a Missing Table** - the series is now removed, matching the PostgreSQL path and the absent-beats-stale rule the rest of this release applies
* **Concurrent `CREATE TABLE` Detection Matched on Message Text** - now matches the SQLSTATE (`42P07` / `23505`) first, keeping the message check only as a fallback for PostgreSQL-compatible servers
* **`CREATE DATABASE` Did Not Validate the Database Name** - a DSN database outside plain-identifier rules failed with a bare syntax error from the server; it now fails with a clear validation message

### Changed
* **Least-Privilege Workflow Permissions** - `build.yml` dropped from `contents: write` to `contents: read` (it never creates a release), and `release.yml` no longer grants `contents: write` and `packages: write` to every job. Each is now granted only where used: `contents: write` on the asset-upload job, `packages: write` on the container job
* **Container Images Pinned Consistently** - `.justfile` used `mariadb:latest` and `mariadb:11` while CI used `mariadb:12`, so a failure could reproduce locally but not in CI, or the reverse. Everything now pins `mariadb:12` and `postgres:18`
* **Removed `CertCache::cleanup()`** - the cache is keyed by `host:port` and dbpulse monitors a single DSN, so it holds one entry and had nothing to reclaim. Expiry is enforced on read. The method was called only from its own test, whose assertions passed with or without it
* **GitHub Actions** - `actions/checkout` updated to `@v7` across all workflows (`security-audit.yml` was already on `@v7`, the rest lagged on `@v6`) and `codecov/codecov-action` to `@v7`. All other actions were already on their latest major
* **Dependencies**
  - `sqlx` updated to `0.9` - the new `SqlSafeStr` trait requires dynamic SQL to be audited and wrapped. All 28 call sites were reviewed and wrapped in `AssertSqlSafe`; only the table name (a hardcoded literal) and the DSN database name are ever interpolated, and every value still binds through a parameter
  - `uuid` updated to `1.26`, plus ~90 transitive updates. Dependency count dropped from 303 to 258
  - Fixed `RUSTSEC-2026-0104` (reachable panic in `rustls-webpki` CRL parsing) and `RUSTSEC-2026-0190`. `sqlx 0.9` drops the `rsa` crate, retiring `RUSTSEC-2023-0071` (Marvin timing attack), which had no upstream fix
* **Error Classification** - a known cause is passed directly instead of being recovered by matching on the error message
  - Certificate-probe failures are classified from the full `anyhow` context chain (`{:#}`) rather than the outermost layer alone, which had made the reason invisible, and the ladder is ordered so a probe timeout is no longer misfiled as a handshake failure

### Tests
* **Hostname-Mismatch TLS Regression Coverage** - `scripts/gen-certs.sh` now also issues a CA-signed certificate naming only `db.invalid`, served by extra PostgreSQL (`:5433`) and MariaDB (`:3307`) containers in `just test-tls` and both CI workflows
  - Asserts that `verify-ca` accepts it *and* still reports certificate expiry, and that `verify-full` still rejects it, on both engines
  - The probe is asserted directly, not only through the health check: on MySQL/MariaDB expiry is backfilled from `Ssl_server_not_after`, which would otherwise hide a failing probe behind a populated metric
  - Verified to fail without the fix and pass with it, on both engines
* **Hostname-Downgrade Unit Tests** - checked-in certificate fixtures exercise the verifier at fixed timestamps, so the tests cannot rot as the wall clock moves, covering: a name mismatch accepted only when configured, an untrusted issuer still rejected, and an expired certificate still rejected
* **Metric Pre-Registration Pinned to Series** - the startup test asserted on bare metric names, which cannot distinguish a partially registered collector. It now asserts whole sample lines including labels and initial values, so a missing `status="success"` child or an unregistered `ERROR_TYPES` entry fails the test

### Technical Details
* Loop deadline, backoff floor, and metric pre-registration in `src/pulse.rs` and `src/metrics.rs`
* Missing-table recovery and cleanup/replication fixes in `src/queries/mysql.rs` and `src/queries/postgres.rs`
* `read_only` flag added to `HealthCheckResult` in `src/queries/mod.rs`
* Probe verification in `src/tls/probe.rs`, using `src/tls/verifier.rs`
* Argument validation in `src/cli/commands/mod.rs`
* New end-to-end regression tests in `tests/timeout_regression_test.rs` covering the wedged-database stall and the absent-metric alerting gap, driving the real binary against a black-hole TCP server
* Concurrent-drop regression tests in `tests/postgres_test.rs` and `tests/mariadb_test.rs`; verified to fail without the fix (12 of 781 checks failed) and pass with it

## 0.9.1 (2026-04-18)

### Changed
* **Dependencies** - Updated core dependencies to their latest versions:
  - `clap` updated to `4.6`
  - `rand` updated to `0.10` (migrated to `RngExt` for `random_range`)
* **GitHub Actions** - Updated CI/CD workflows to latest 2026 versions:
  - Migrated actions to `@v6`, `@v7`, and `@v1` tags supporting Node 24 runtime
  - Updated MariaDB service images to `mariadb:12`
* **Code Quality** - Comprehensive linting and formatting pass:
  - Fixed all `clippy` warnings (collapsed matches, suboptimal duration units)
  - Applied standard project formatting with `cargo fmt`

## 0.9.0 (2026-02-13)

### Breaking Changes
* **Pulse Semantics in Read-Only/Recovery** - `dbpulse_pulse` now reports unhealthy (`0`) when writes are not possible
  - **Previous behavior**: Database could still appear healthy while in read-only/recovery state
  - **New behavior**: Read-only/recovery conditions are treated as failed health checks
  - **Impact**: Alerting/SLOs based on `dbpulse_pulse` or `dbpulse_iterations_total` may change
  - **Rationale**: `dbpulse` health checks are intended to validate end-to-end read/write/transaction capability

### Added
* **Database Host Metric** - New metric: `dbpulse_database_host_info{database,host}=1`
  - Exposes backend host identity for operators during VIP/failover transitions
  - MySQL/MariaDB source: `SELECT @@hostname`
  - PostgreSQL source: `SELECT COALESCE(inet_server_addr()::text, 'local')`
  - Enables clear dashboard visibility of which backend is serving traffic
* **Failover Transition Integration Tests**
  - Added PostgreSQL and MariaDB transition tests validating pulse sequence `1 -> 0 -> 1` during stop/start events
  - Tests simulate failover-like interruption and recovery at the exporter layer
  - Environment-gated locally with `RUN_FAILOVER_TRANSITION_TESTS=1`
* **CI Failover Coverage**
  - Added dedicated GitHub Actions job to always run failover transition tests in CI
  - Runs transition tests in isolation with `RUN_FAILOVER_TRANSITION_TESTS=1` to prevent interference with regular integration tests

### Fixed
* **Sub-Second Interval Accuracy** - Fixed sleep timing regression for short intervals (issue #11)
  - **Root Cause**: Remaining interval sleep was truncated to whole seconds
  - **Impact**: `-i 1` and other short intervals could run faster than configured
  - **Solution**: Preserved sub-second remainder when calculating sleep duration
  - Added regression tests for millisecond-level remainder handling
* **Database Version Info Label Staleness**
  - Fixed stale `dbpulse_database_version_info` label series persisting across transitions
  - Previous version label values are now removed when version changes
  - Prevents dashboards from showing old and new version labels simultaneously

### Technical Details
* Pulse/read-only behavior updates in `src/pulse.rs`
* Host collection added in:
  - `src/queries/mysql.rs`
  - `src/queries/postgres.rs`
  - `src/queries/mod.rs`
* New metric definition in `src/metrics.rs`
* Dashboard updates in `grafana/dashboard.json` (Database Host panel + Database Version handling)
* New integration and helper coverage in:
  - `tests/postgres_test.rs`
  - `tests/mariadb_test.rs`
  - `tests/common/mod.rs`
* CI failover workflow coverage added in `.github/workflows/test.yml`

## 0.8.3 (2025-11-21)

### Fixed
* **PostgreSQL Table Schema** - Fixed `SERIAL PRIMARY KEY` conflict with manual ID insertion
  - **Root Cause**: Table used `SERIAL PRIMARY KEY` while code manually inserts random IDs
  - **Impact**: Auto-incrementing sequence could drift out of sync with manual inserts, causing future conflicts
  - **Solution**: Changed from `id SERIAL PRIMARY KEY` to `id INT NOT NULL PRIMARY KEY`
  - Now consistent with MySQL/MariaDB implementation
  - Prevents sequence-related conflicts in long-running deployments
* **PostgreSQL Row Count Query** - Fixed potential incorrect row count with multiple schemas
  - **Root Cause**: Query lacked schema qualification when selecting from `pg_class`
  - **Impact**: Could return wrong table statistics if multiple schemas have tables with the same name
  - **Solution**: Added schema qualification with `pg_namespace` join
  - Query now uses: `JOIN pg_namespace n ON c.relnamespace = n.oid WHERE ... AND n.nspname = CURRENT_SCHEMA()`
  - Ensures row count metrics always reference the correct table in the current schema

### Added
* **Metrics Collection Verification Tests** - New integration tests validate metric population
  - `test_postgres_metrics_collection`: Validates PostgreSQL metrics are collected and exposed
  - `test_mariadb_metrics_collection`: Validates MariaDB metrics are collected and exposed
  - Tests verify critical metrics: `dbpulse_operation_duration_seconds`, `dbpulse_rows_affected_total`, `dbpulse_connection_duration_seconds`
  - Tests verify database-specific operation labels (connect, insert, select)
  - Tests confirm database size and table size metrics are being populated
  - Total: 2 new integration tests ensuring metrics work end-to-end

### Technical Details
* PostgreSQL table schema fix in `src/queries/postgres.rs:269`
* PostgreSQL row count query fix in `src/queries/postgres.rs:428-432`
* New tests in `tests/postgres_test.rs` and `tests/mariadb_test.rs`
* All changes maintain backward compatibility - no schema migrations needed
* Existing deployments: table will be recreated with correct schema during next periodic drop
* Zero breaking changes for existing users
* All quality gates passed: fmt, clippy (strict), tests (97 unit + 23 integration + 15 TLS)

## 0.8.2 (2025-11-19)

### Fixed
* **MariaDB Type Compatibility** - Fixed metrics not populating due to type mismatches
  - **Root Cause**: MariaDB uses `BIGINT UNSIGNED` and `DECIMAL` types in `information_schema.TABLES`, while MySQL uses `BIGINT`
  - **Impact**: `dbpulse_table_size_bytes`, `dbpulse_table_rows`, and `dbpulse_database_size_bytes` were not populating for MariaDB
  - **Solution**: Added `CAST(... AS SIGNED)` to handle type differences between MySQL and MariaDB
  - Table size query: `CAST(COALESCE(data_length, 0) + COALESCE(index_length, 0) AS SIGNED)`
  - Table rows query: `CAST(table_rows AS SIGNED)` with fallback to `COUNT(*)`
  - Database size query: `CAST(SUM(COALESCE(...)) AS SIGNED)`
  - Added `COALESCE()` for NULL handling when statistics aren't initialized
  - Added fallback to exact `COUNT(*)` when `information_schema` returns NULL
  - Added error logging to stderr for debugging type mismatches
  - All three metrics now populate correctly for both MySQL and MariaDB

### Added
* **Comprehensive Metrics Validation Tests** - New integration test suite (388 lines)
  - `test_postgres_all_metrics_present`: Validates all 9 query function metrics for PostgreSQL
  - `test_mariadb_all_metrics_present`: Validates all 9 query function metrics for MariaDB
  - `test_postgres_and_mariadb_metric_parity`: Ensures consistent behavior between databases
  - Tests verify non-zero values, not just metric presence
  - Clear assertions showing which bugs were fixed
  - Runs with real Podman/Docker containers for 100% validation
  - Validates the MariaDB type compatibility fixes
  - Ensures no regressions in future changes
  - Total test coverage: 100% of query function metrics

### Technical Details
* MariaDB type compatibility fixes in `src/queries/mysql.rs`
* New test file: `tests/metrics_validation_test.rs` (388 lines)
* All changes maintain backward compatibility with Prometheus queries
* Zero breaking changes for existing deployments
* Code changes: 442 lines total (431 insertions, 11 deletions)
* Validated with real MariaDB and PostgreSQL containers
* All quality gates passed: fmt, clippy (strict), tests (97 unit + 3 integration)

## 0.8.1 (2025-11-19)

### Fixed
* **MySQL Table Size Metric** - Fixed `dbpulse_table_size_bytes` not populating for MySQL/MariaDB
  - Changed from using bind parameter (`.bind(table_name)`) to string interpolation (`format!()`)
  - Bind parameters don't work for metadata queries against `information_schema.TABLES`
  - Now consistent with PostgreSQL implementation pattern
  - Table size metric now properly displays in Grafana for MySQL databases
* **Table Row Count Frequency** - Updated to populate on every health check instead of once per hour
  - Previously: `dbpulse_table_rows` only updated during hourly cleanup (minute 0 with id < 5)
  - Now: Updates on every health check using fast approximate counts
  - MySQL: Uses `information_schema.TABLES.table_rows` (InnoDB statistics)
  - PostgreSQL: Uses `pg_class.reltuples` (table statistics)
  - No performance impact - both use fast estimates without table scans
  - Better monitoring experience with up-to-date row count trends

### Removed
* **CONNECTIONS_ACTIVE Metric** - Removed useless metric that always showed 0 or 1
  - Removed `dbpulse_connections_active` metric definition from code
  - Reason: Sequential health checks mean the metric only ever shows 0 or 1
  - Per-instance memory means multiple instances don't aggregate meaningfully
  - The metric provided no useful observability value
  - Cleaned up 41 lines of unnecessary code (metric definition, increment/decrement calls, tests)

### Improved
* **Grafana Dashboard Optimization** - Removed redundant panels and improved metric coverage
  - Removed "Active Connections" panel (metric no longer exists)
  - Removed duplicate "Blocking Queries" gauge panel (kept the stat panel)
  - Moved "Database Size" panel to Overview section (position #6)
  - Replaced "Certificate Expiry Timeline" with "TLS Certificate Probe Errors" panel
  - New panel shows certificate probing failures by type (connection, handshake, parse, timeout)
  - Total panels: 26 (down from 28)
  - Metrics coverage: 100% (all 23 metrics now used in dashboard)
  - No unused metrics, no duplicate panels
  - Dashboard JSON reduced from 2828 to 2670 lines
* **Code Documentation** - Fixed misleading comments about cleanup behavior
  - Updated comments in `mysql.rs` and `postgres.rs` to reflect actual probabilistic cleanup
  - Changed "deterministic cleanup every hour" to "probabilistic cleanup at minute 0"
  - Clarified that `id < 5` condition (5/range probability) prevents simultaneous drops
  - Explains the entropy-based approach for distributed coordination-free cleanup
  - Primary cleanup (delete old records) runs on every health check as documented

### Technical Details
* Table size query fix applies to line 432 in `src/queries/mysql.rs`
* Table row count updates now on lines 403-414 (MySQL) and 426-437 (PostgreSQL)
* Dashboard changes: 183 lines modified (22 insertions, 161 deletions)
* All changes maintain backward compatibility with Prometheus queries
* Zero breaking changes for existing deployments

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
