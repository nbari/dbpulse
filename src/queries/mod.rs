pub mod mysql;
pub mod postgres;

use crate::{metrics::REPLICATION_LAG, tls::TlsMetadata};

/// Classify a certificate-probe failure for `dbpulse_tls_cert_probe_errors_total`.
///
/// Renders the error once, through `{:#}` so the whole `.context()` chain is
/// considered rather than only the outermost layer, and shares one ordering
/// between both drivers instead of two hand-copied ladders.
///
/// Order matters and is deliberate: an unreachable host is a reachability
/// problem even when the OS phrases it as "Connection timed out", while the
/// probe's own deadline is a timeout even though its message names TLS. The
/// previous ladder tested `contains("TLS")` for "handshake" before it tested
/// for a timeout, so every probe timeout would have been filed as a handshake
/// failure.
pub(crate) fn classify_cert_probe_error(error: &anyhow::Error) -> &'static str {
    let rendered = format!("{error:#}");

    if rendered.contains("connect") || rendered.contains("Connection") {
        "connection"
    } else if rendered.contains("timed out") || rendered.contains("timeout") {
        "timeout"
    } else if rendered.contains("handshake") {
        "handshake"
    } else if rendered.contains("parse") || rendered.contains("certificate") {
        "parse"
    } else {
        "unknown"
    }
}

/// Publish a gauge sample, or retire the series when there is no value.
///
/// A gauge that stops being written keeps its last sample forever, so a
/// best-effort collector that starts failing goes on reporting a stale reading
/// as though it were current. Every such collector funnels through here so the
/// "absent beats stale" rule cannot be forgotten at a new call site.
pub(crate) fn set_or_retire(gauge: &prometheus::IntGaugeVec, labels: &[&str], value: Option<i64>) {
    match value {
        Some(v) => gauge.with_label_values(labels).set(v),
        None => {
            let _ = gauge.remove_label_values(labels);
        }
    }
}

/// Record replication lag, or drop the series when there is nothing to report.
///
/// `dbpulse_replication_lag_seconds` is a gauge, so an un-updated series keeps
/// its last value forever. Leaving a stale sample behind after a promotion or a
/// stopped replica is worse than reporting nothing: the metric would claim a
/// healthy lag for a node that is no longer replicating at all, and
/// `... > 30` style alerts would never fire. Removing the child makes the
/// series absent, which `absent()` and `unless` can both detect.
pub(crate) fn record_replication_lag(database: &str, lag_seconds: Option<i64>) {
    match lag_seconds {
        Some(seconds) if seconds >= 0 => {
            REPLICATION_LAG.with_label_values(&[database]).set(seconds);
        }
        _ => {
            let _ = REPLICATION_LAG.remove_label_values(&[database]);
        }
    }
}

/// Result from database health check including TLS metadata
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// Database version string
    pub version: String,
    /// Database host currently serving the connection (if available)
    pub db_host: Option<String>,
    /// Database uptime in seconds (if available)
    pub uptime_seconds: Option<i64>,
    /// TLS metadata (if TLS is enabled)
    pub tls_metadata: Option<TlsMetadata>,
    /// Whether the server reported that it is in read-only mode.
    ///
    /// Carried as a flag rather than encoded into `version`: the version string
    /// is used as a Prometheus label, and mixing volatile state into a label
    /// churns the series every time the database flips read-only.
    pub read_only: bool,
    /// Why the server is read-only, when it is (recovery mode, transaction
    /// read-only, ...). Reported in the JSON pulse line for operators; never
    /// used as a metric label.
    pub read_only_reason: Option<String>,
}

/// Outcome of reading back the row the read/write check just wrote.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReadBack {
    /// Exactly what was written came back.
    Match,
    /// A different instance overwrote the row between the write and the read.
    ///
    /// Not a database fault. Instances share one table and `--range` does not
    /// partition the ID space (every range starts at zero), so two monitors
    /// eventually pick the same ID. The row carries a timestamp at or after
    /// ours, which is what a later writer looks like.
    ConcurrentOverwrite,
    /// The row came back with data older than what was written, which no
    /// concurrent writer can explain: the write was lost, or the read was
    /// served stale. This is the condition the check exists to catch.
    Mismatch,
}

/// Classify the row read back after the upsert.
///
/// Comparing for exact equality alone made a second dbpulse instance look like
/// data corruption. Distinguishing the two matters because they demand opposite
/// responses: a concurrent overwrite is expected on a shared table, whereas a
/// stale or lost write means the database is not honouring its own writes.
pub(crate) fn classify_read_back(
    written_t1: i64,
    written_uuid: &str,
    read_t1: i64,
    read_uuid: &str,
) -> ReadBack {
    if written_t1 == read_t1 && written_uuid == read_uuid {
        ReadBack::Match
    } else if read_t1 >= written_t1 {
        ReadBack::ConcurrentOverwrite
    } else {
        ReadBack::Mismatch
    }
}

/// Reject anything that is not a plain SQL identifier.
///
/// The read/write check builds its DDL with `format!` and hands the result to
/// `AssertSqlSafe`, which -- despite the name -- performs no escaping
/// whatsoever: it is an assertion by the caller, not a check. In the binary the
/// table name is the hardcoded literal `dbpulse_rw`, so nothing is exploitable
/// today, but `test_rw_with_table` is a public API and the next caller has no
/// way to know that the string it passes is spliced directly into `DROP TABLE`.
///
/// Validating instead of quoting keeps this engine-agnostic: the accepted set
/// is unquoted-safe in both PostgreSQL and MySQL, and the only name production
/// uses is already within it.
pub(crate) fn validate_identifier(kind: &str, name: &str) -> anyhow::Result<()> {
    if name.is_empty() {
        anyhow::bail!("{kind} must not be empty");
    }

    // 63 is PostgreSQL's NAMEDATALEN-1 limit; MySQL allows 64. Hold both to the
    // stricter one: PostgreSQL silently *truncates* a longer name rather than
    // erroring, so a 64-character name would be accepted here and then refer to
    // a different object than the one asked for.
    if name.len() > 63 {
        anyhow::bail!("{kind} `{name}` is longer than 63 characters");
    }

    if name.starts_with(|c: char| c.is_ascii_digit()) {
        anyhow::bail!("{kind} `{name}` must not start with a digit");
    }

    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
    {
        anyhow::bail!("{kind} `{name}` contains an unsupported character `{bad}`");
    }

    Ok(())
}

/// Bound the id space the read/write check samples from.
///
/// `random_range(0..0)` panics, and both schemas store the id in a signed
/// `INT`, so values above `i32::MAX` cannot be written at all. The CLI
/// rejects both at parse time; this keeps the public
/// [`mysql::test_rw_with_table`] / [`postgres::test_rw_with_table`] entry
/// points from panicking -- or failing roughly every other iteration -- for
/// library callers.
pub(crate) fn validate_range(range: u32) -> anyhow::Result<()> {
    if range == 0 {
        anyhow::bail!("range must be at least 1");
    }
    if i64::from(range) > i64::from(i32::MAX) {
        anyhow::bail!(
            "range {range} exceeds the signed INT column maximum of {}",
            i32::MAX
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    /// Regression: a second dbpulse instance overwriting the row must not be
    /// reported as a data fault.
    ///
    /// Instances share one table and `--range` does not partition the ID space
    /// -- every range starts at zero -- so two monitors eventually pick the
    /// same ID. Comparing for exact equality alone turned that into "Records
    /// don't match", indistinguishable from the database losing a write.
    #[test]
    fn a_concurrent_overwrite_is_not_a_mismatch() {
        // Another instance wrote the same row a second later.
        assert_eq!(
            classify_read_back(1_000, "ours", 1_001, "theirs"),
            ReadBack::ConcurrentOverwrite
        );
        // Or within the same second, which is the common case at 30s intervals.
        assert_eq!(
            classify_read_back(1_000, "ours", 1_000, "theirs"),
            ReadBack::ConcurrentOverwrite
        );
    }

    /// The check must still catch what it exists to catch: a row that comes
    /// back older than what was written means the write was lost or the read
    /// was served stale, which no concurrent writer can explain.
    #[test]
    fn a_stale_read_is_still_a_mismatch() {
        assert_eq!(
            classify_read_back(1_000, "ours", 999, "theirs"),
            ReadBack::Mismatch
        );
        assert_eq!(
            classify_read_back(1_000, "ours", 500, "ours"),
            ReadBack::Mismatch
        );
    }

    #[test]
    fn an_exact_read_back_matches() {
        assert_eq!(
            classify_read_back(1_000, "ours", 1_000, "ours"),
            ReadBack::Match
        );
    }

    /// A table name is spliced straight into `DROP TABLE` via `AssertSqlSafe`,
    /// which does no escaping. Anything that could terminate the identifier and
    /// begin a new statement, or comment out the rest of one, must be refused.
    #[test]
    fn identifier_validation_rejects_injection_attempts() {
        for name in [
            "dbpulse_rw; DROP TABLE users",
            "dbpulse_rw--",
            "dbpulse_rw\"",
            "dbpulse_rw`",
            "dbpulse rw",
            "dbpulse_rw'",
            "public.dbpulse_rw",
            "",
            "1_starts_with_digit",
        ] {
            assert!(
                validate_identifier("table name", name).is_err(),
                "`{name}` must be rejected before it reaches the query builder"
            );
        }
    }

    #[test]
    fn identifier_validation_accepts_ordinary_names() {
        for name in ["dbpulse_rw", "T", "_leading_underscore", "mixedCase123"] {
            assert!(
                validate_identifier("table name", name).is_ok(),
                "`{name}` is a plain identifier and must be accepted"
            );
        }
    }

    /// Regression: the limit was 64, but PostgreSQL truncates at 63 rather than
    /// erroring, so a 64-character name passed validation and then silently
    /// referred to a different object than the one asked for.
    #[test]
    fn identifier_validation_rejects_over_long_names() {
        assert!(validate_identifier("table name", &"a".repeat(63)).is_ok());
        assert!(validate_identifier("table name", &"a".repeat(64)).is_err());
    }

    /// Regression: `random_range(0..0)` panicked on every iteration, and the
    /// panic handler simply re-entered the loop forever without making
    /// progress. The CLI rejects `--range 0`; library callers must get an
    /// error instead of a panic.
    #[test]
    fn range_zero_is_rejected() {
        assert!(validate_range(0).is_err());
        assert!(validate_range(1).is_ok());
    }

    /// The id column is a signed `INT` in both schemas, so ids above
    /// `i32::MAX` cannot be written: PostgreSQL errored each such iteration
    /// (`generated id out of range`), MySQL with server error 1264.
    #[test]
    fn range_is_capped_at_the_int_column_maximum() {
        assert!(validate_range(2_147_483_647).is_ok());
        assert!(validate_range(2_147_483_648).is_err());
    }

    #[test]
    fn test_health_check_result_without_tls() {
        let result = HealthCheckResult {
            version: "PostgreSQL 15.0".to_string(),
            db_host: Some("db-1".to_string()),
            uptime_seconds: Some(1_000),
            tls_metadata: None,
            read_only: false,
            read_only_reason: None,
        };

        assert_eq!(result.version, "PostgreSQL 15.0");
        assert_eq!(result.db_host, Some("db-1".to_string()));
        assert_eq!(result.uptime_seconds, Some(1_000));
        assert!(result.tls_metadata.is_none());
    }

    #[test]
    fn test_health_check_result_with_tls() {
        let tls_metadata = TlsMetadata {
            version: Some("TLSv1.3".to_string()),
            cipher: Some("AES256-GCM-SHA384".to_string()),
            cert_subject: None,
            cert_issuer: None,
            cert_expiry_days: None,
        };

        let result = HealthCheckResult {
            version: "MySQL 8.0.33".to_string(),
            db_host: Some("db-2".to_string()),
            uptime_seconds: Some(42),
            tls_metadata: Some(tls_metadata),
            read_only: false,
            read_only_reason: None,
        };

        assert_eq!(result.version, "MySQL 8.0.33");
        assert_eq!(result.db_host, Some("db-2".to_string()));
        assert_eq!(result.uptime_seconds, Some(42));
        assert!(result.tls_metadata.is_some());
        let tls = result.tls_metadata.as_ref().unwrap();
        assert_eq!(tls.version.as_ref().unwrap(), "TLSv1.3");
        assert_eq!(tls.cipher.as_ref().unwrap(), "AES256-GCM-SHA384");
    }

    #[test]
    fn test_health_check_result_clone() {
        let result = HealthCheckResult {
            version: "PostgreSQL 14.5".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
            read_only: false,
            read_only_reason: None,
        };

        let cloned = result.clone();
        assert_eq!(cloned.version, result.version);
        assert_eq!(cloned.uptime_seconds, result.uptime_seconds);
        assert!(cloned.tls_metadata.is_none());
    }

    #[test]
    fn test_health_check_result_debug() {
        let result = HealthCheckResult {
            version: "MySQL 8.0".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
            read_only: false,
            read_only_reason: None,
        };

        let debug_str = format!("{result:?}");
        assert!(debug_str.contains("HealthCheckResult"));
        assert!(debug_str.contains("MySQL 8.0"));
    }

    #[test]
    fn test_health_check_result_empty_version() {
        let result = HealthCheckResult {
            version: String::new(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
            read_only: false,
            read_only_reason: None,
        };

        assert_eq!(result.version, "");
        assert!(result.tls_metadata.is_none());
    }

    #[test]
    fn test_health_check_result_with_full_tls_metadata() {
        let tls_metadata = TlsMetadata {
            version: Some("TLSv1.2".to_string()),
            cipher: Some("ECDHE-RSA-AES128-GCM-SHA256".to_string()),
            cert_subject: Some("CN=db.example.com".to_string()),
            cert_issuer: Some("CN=Example CA".to_string()),
            cert_expiry_days: Some(90),
        };

        let result = HealthCheckResult {
            version: "PostgreSQL 13.0".to_string(),
            db_host: Some("replica-1".to_string()),
            uptime_seconds: Some(900),
            tls_metadata: Some(tls_metadata),
            read_only: true,
            read_only_reason: Some("Database is in recovery mode".to_string()),
        };

        // Regression: recovery state travels in `read_only`/`read_only_reason`,
        // never appended to `version`. The version string is a Prometheus label
        // and must stay a stable identity, not a carrier for volatile state.
        assert_eq!(result.version, "PostgreSQL 13.0");
        assert!(result.read_only);
        assert_eq!(
            result.read_only_reason.as_deref(),
            Some("Database is in recovery mode")
        );
        assert_eq!(result.db_host, Some("replica-1".to_string()));
        let tls = result.tls_metadata.as_ref().unwrap();
        assert_eq!(tls.cert_subject.as_ref().unwrap(), "CN=db.example.com");
        assert_eq!(tls.cert_issuer.as_ref().unwrap(), "CN=Example CA");
        assert_eq!(tls.cert_expiry_days.unwrap(), 90);
    }

    /// Regression: a read-only MySQL server reports the flag, and its version
    /// stays exactly what the server returned.
    #[test]
    fn test_health_check_result_mysql_read_only() {
        let result = HealthCheckResult {
            version: "MySQL 8.0.30".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
            read_only: true,
            read_only_reason: Some("Database is in read-only mode".to_string()),
        };

        assert!(result.read_only);
        assert_eq!(
            result.read_only_reason.as_deref(),
            Some("Database is in read-only mode")
        );
        assert!(
            !result.version.contains("read-only"),
            "read-only state must not be encoded into the version label"
        );
    }

    #[test]
    fn test_health_check_result_version_with_special_chars() {
        let result = HealthCheckResult {
            version: "PostgreSQL 15.0 (Ubuntu 15.0-1.pgdg22.04+1)".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
            read_only: false,
            read_only_reason: None,
        };

        assert!(result.version.contains("Ubuntu"));
        assert!(result.version.contains("pgdg22.04"));
    }

    fn lag_series_count(database: &str) -> usize {
        crate::metrics::REGISTRY
            .gather()
            .into_iter()
            .find(|family| family.name() == "dbpulse_replication_lag_seconds")
            .map_or(0, |family| {
                family
                    .get_metric()
                    .iter()
                    .filter(|metric| {
                        metric
                            .get_label()
                            .iter()
                            .any(|lp| lp.name() == "database" && lp.value() == database)
                    })
                    .count()
            })
    }

    /// Regression: replication lag is a gauge, so a value that stops being
    /// refreshed keeps claiming a healthy lag forever. After a promotion, or
    /// once replication halts, the series must go absent instead.
    #[test]
    fn replication_lag_series_is_dropped_when_there_is_nothing_to_report() {
        let database = "test-lag-promotion";

        record_replication_lag(database, Some(7));
        assert_eq!(lag_series_count(database), 1);

        record_replication_lag(database, None);
        assert_eq!(
            lag_series_count(database),
            0,
            "a halted or promoted replica must not keep reporting its last lag"
        );
    }

    /// `SHOW REPLICA STATUS` reports -1 on some servers when replication is
    /// not running; that is not a lag of minus one second.
    #[test]
    fn replication_lag_ignores_negative_values() {
        let database = "test-lag-negative";

        record_replication_lag(database, Some(3));
        assert_eq!(lag_series_count(database), 1);

        record_replication_lag(database, Some(-1));
        assert_eq!(lag_series_count(database), 0);
    }

    #[test]
    fn replication_lag_records_zero_as_a_real_sample() {
        let database = "test-lag-zero";

        record_replication_lag(database, Some(0));
        assert_eq!(
            lag_series_count(database),
            1,
            "a fully caught-up replica reports 0, which is a value, not an absence"
        );
    }

    /// Regression: the probe's own deadline must be filed as a timeout. The
    /// previous ladder tested for "TLS" before it tested for a timeout, so
    /// this message would have been counted as a handshake failure.
    #[test]
    fn probe_timeout_is_classified_as_a_timeout() {
        let error = anyhow::anyhow!("TLS certificate probe timed out after 3s (Postgres)");
        assert_eq!(classify_cert_probe_error(&error), "timeout");
    }

    #[test]
    fn probe_error_classification_covers_each_stage() {
        let unreachable = anyhow::anyhow!(
            "failed to connect to db:5432 for TLS certificate probe (protocol: Postgres)"
        );
        assert_eq!(classify_cert_probe_error(&unreachable), "connection");

        let handshake =
            anyhow::anyhow!("failed to complete TLS handshake for certificate probe (Postgres)");
        assert_eq!(classify_cert_probe_error(&handshake), "handshake");

        let parse = anyhow::anyhow!("failed to extract certificate metadata from TLS stream");
        assert_eq!(classify_cert_probe_error(&parse), "parse");

        let unknown = anyhow::anyhow!("something else entirely");
        assert_eq!(classify_cert_probe_error(&unknown), "unknown");
    }

    /// Regression: classification renders `{:#}`, so a cause wrapped by
    /// `.context()` is still visible. `to_string()` shows only the outermost
    /// layer, which would file this failure as "unknown".
    #[test]
    fn probe_error_classification_sees_the_whole_context_chain() {
        let error = anyhow::anyhow!("handshake aborted").context("certificate probe failed");

        assert_eq!(
            error.to_string(),
            "certificate probe failed",
            "the outermost layer alone carries no stage information"
        );
        assert_eq!(classify_cert_probe_error(&error), "handshake");
    }
}
