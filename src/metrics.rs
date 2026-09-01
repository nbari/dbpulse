use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Registry, opts, register_histogram_vec_with_registry,
    register_histogram_with_registry, register_int_counter_vec_with_registry,
    register_int_counter_with_registry, register_int_gauge_vec_with_registry,
    register_int_gauge_with_registry,
};
use std::sync::LazyLock;

pub static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

/// Registration only fails on a duplicate or malformed metric name, both of
/// which are fixed string literals here -- so a failure means the binary was
/// built wrong, not that something went wrong at runtime. Exiting keeps the
/// statics infallible without `unwrap`/`expect`, which the crate lints deny.
///
/// The one way to trigger it from outside is to register a conflicting name
/// into the public [`REGISTRY`] before any dbpulse metric is first touched.
trait ResultExt<T> {
    fn or_exit(self, context: &str) -> T;
}

impl<T, E> ResultExt<T> for Result<T, E>
where
    E: std::fmt::Display,
{
    fn or_exit(self, context: &str) -> T {
        match self {
            Ok(value) => value,
            Err(err) => {
                eprintln!("failed to initialize metric ({context}): {err}");
                std::process::exit(1);
            }
        }
    }
}

pub static PULSE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge_with_registry!("dbpulse_pulse", "1 ok, 0 error", &REGISTRY)
        .or_exit("metric can be created")
});

pub static RUNTIME: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram_with_registry!(
        HistogramOpts::new("dbpulse_runtime", "pulse latency in seconds"),
        &REGISTRY
    )
    .or_exit("metric can be created")
});

// TLS-specific metrics
pub static TLS_HANDSHAKE_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec_with_registry!(
        HistogramOpts::new(
            "dbpulse_tls_handshake_duration_seconds",
            "TLS handshake duration in seconds"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static TLS_CONNECTION_ERRORS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_tls_connection_errors_total",
            "Total TLS connection errors by type"
        ),
        &["database", "error_type"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static TLS_INFO: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_tls_info",
            "TLS connection info (version, cipher) - value is always 1"
        ),
        &["database", "version", "cipher"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static TLS_CERT_EXPIRY_DAYS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_tls_cert_expiry_days",
            "Days until TLS certificate expiration (negative if expired)"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static TLS_CERT_PROBE_ERRORS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_tls_cert_probe_errors_total",
            "Total certificate probe errors by type (connection, handshake, parse, timeout)"
        ),
        &["database", "error_type"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static DATABASE_VERSION_INFO: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_database_version_info",
            "Database server version info (value is always 1)"
        ),
        &["database", "version"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static DATABASE_HOST_INFO: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_database_host_info",
            "Database host currently serving the connection (value is always 1)"
        ),
        &["database", "host"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static DATABASE_UPTIME_SECONDS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_database_uptime_seconds",
            "How long (in seconds) the database has been up"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static LAST_RUNTIME_MS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_runtime_last_milliseconds",
            "Runtime of the most recent health check iteration in milliseconds"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

// Critical Priority Metrics
pub static DB_ERRORS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!("dbpulse_errors_total", "Total database errors by type"),
        &["database", "error_type"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static OPERATION_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    register_histogram_vec_with_registry!(
        HistogramOpts::new(
            "dbpulse_operation_duration_seconds",
            "Duration of specific database operations"
        ),
        &["database", "operation"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static CONNECTION_DURATION: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram_with_registry!(
        HistogramOpts::new(
            "dbpulse_connection_duration_seconds",
            "Time connection is held open"
        ),
        &REGISTRY
    )
    .or_exit("metric can be created")
});

// High Priority Metrics
pub static ROWS_AFFECTED: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_rows_affected_total",
            "Total rows affected by operations"
        ),
        &["database", "operation"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static ITERATIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!("dbpulse_iterations_total", "Total monitoring iterations"),
        &["database", "status"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static LAST_SUCCESS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_last_success_timestamp_seconds",
            "Unix timestamp of last successful check"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

// Medium Priority Metrics
pub static TABLE_SIZE_BYTES: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_table_size_bytes",
            "Approximate table size in bytes"
        ),
        &["database", "table"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static TABLE_ROWS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!("dbpulse_table_rows", "Approximate row count"),
        &["database", "table"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static TABLE_RECREATED: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_table_recreated_total",
            "Times the monitoring table vanished mid-check and was recreated"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

/// Times the read-back row had been overwritten by another dbpulse instance.
///
/// Instances share one table and `--range` does not partition the ID space, so
/// two monitors eventually choose the same row. That is expected on a
/// multi-instance deployment and is not a database fault, but it is worth
/// seeing: a high rate means `--range` is too small for the number of
/// instances, and a nonzero value on a single-instance deployment means
/// something else is writing to the table.
pub static RW_ROW_CONTENTION: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_rw_row_contention_total",
            "Times the read/write check row was overwritten by another writer"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

/// Failures of the best-effort hourly table maintenance.
///
/// The `DROP TABLE` that keeps the monitoring table from growing without bound
/// is deliberately not fatal -- a monitor that stops reporting because it could
/// not tidy up is worse than one carrying a slightly large table. But
/// discarding the error entirely means a permission change or a lock that
/// blocks maintenance forever is invisible until the table has grown large
/// enough to cause a real outage. Count it so it can be alerted on.
pub static TABLE_MAINTENANCE_ERRORS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_table_maintenance_errors_total",
            "Failures of the periodic monitoring-table maintenance, by operation"
        ),
        &["database", "operation"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static PANICS_RECOVERED: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter_with_registry!(
        opts!(
            "dbpulse_panics_recovered_total",
            "Total panics recovered from"
        ),
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static DB_READONLY: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_database_readonly",
            "1 if database is in read-only mode"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

// Replication and Performance Metrics
pub static REPLICATION_LAG: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_replication_lag_seconds",
            "Replication lag in seconds (for replicas)"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

/// Sessions currently *waiting* on a lock.
///
/// Named for what the query measures. It counts the victims of contention, not
/// the culprits: PostgreSQL selects `wait_event_type = 'Lock'` and MySQL
/// selects processlist states matching `lock`, both of which are sessions that
/// are stuck. A single blocking transaction can produce many blocked sessions,
/// so reading this as "queries blocking others" overstates the cause and
/// understates nothing.
pub static BLOCKED_SESSIONS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_blocked_sessions",
            "Number of sessions currently waiting on a lock"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

pub static DATABASE_SIZE_BYTES: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_database_size_bytes",
            "Total database size in bytes"
        ),
        &["database"],
        &REGISTRY
    )
    .or_exit("metric can be created")
});

/// Error types reported on `dbpulse_errors_total`.
///
/// Pre-created at startup so the counters exist at zero rather than appearing
/// only after the first failure of that kind. Kept in sync with
/// `pulse::classify_error_type`, which is asserted by a unit test there.
pub const ERROR_TYPES: [&str; 5] = [
    "authentication",
    "timeout",
    "connection",
    "transaction",
    "query",
];

/// Operations reported on `dbpulse_table_maintenance_errors_total`.
///
/// Pre-created at startup for the same reason as [`ERROR_TYPES`]: a counter
/// that only springs into existence on first failure cannot be alerted on with
/// a rate/increase expression until it has already failed once.
pub const TABLE_MAINTENANCE_OPERATIONS: [&str; 2] = ["count", "drop"];

/// Force registration of every metric with the registry.
///
/// Each metric is a `LazyLock`, so it is only registered with `REGISTRY` the
/// first time it is dereferenced. Without this, a metric that has not been
/// touched yet is *absent* from `/metrics` rather than reported as zero, and an
/// alert written as `dbpulse_pulse == 0` silently never fires: absent series do
/// not match a comparison. Call this once at startup so the full metric surface
/// exists from the very first scrape, before any health check has completed.
pub fn init(database: Option<&str>) {
    LazyLock::force(&PULSE);
    LazyLock::force(&RUNTIME);
    LazyLock::force(&TLS_HANDSHAKE_DURATION);
    LazyLock::force(&TLS_CONNECTION_ERRORS);
    LazyLock::force(&TLS_INFO);
    LazyLock::force(&TLS_CERT_EXPIRY_DAYS);
    LazyLock::force(&TLS_CERT_PROBE_ERRORS);
    LazyLock::force(&DATABASE_VERSION_INFO);
    LazyLock::force(&DATABASE_HOST_INFO);
    LazyLock::force(&DATABASE_UPTIME_SECONDS);
    LazyLock::force(&LAST_RUNTIME_MS);
    LazyLock::force(&DB_ERRORS);
    LazyLock::force(&OPERATION_DURATION);
    LazyLock::force(&CONNECTION_DURATION);
    LazyLock::force(&ROWS_AFFECTED);
    LazyLock::force(&ITERATIONS_TOTAL);
    LazyLock::force(&LAST_SUCCESS);
    LazyLock::force(&TABLE_SIZE_BYTES);
    LazyLock::force(&TABLE_ROWS);
    LazyLock::force(&TABLE_RECREATED);
    LazyLock::force(&TABLE_MAINTENANCE_ERRORS);
    LazyLock::force(&RW_ROW_CONTENTION);
    LazyLock::force(&PANICS_RECOVERED);
    LazyLock::force(&DB_READONLY);
    LazyLock::force(&REPLICATION_LAG);
    LazyLock::force(&BLOCKED_SESSIONS);
    LazyLock::force(&DATABASE_SIZE_BYTES);

    // Forcing a *Vec registers the collector but exports no lines: a labelled
    // metric only appears once a label combination exists. Create the children
    // for the database being monitored so alerts have something to match from
    // the first scrape.
    let Some(database) = database else {
        return;
    };
    ITERATIONS_TOTAL.with_label_values(&[database, "success"]);
    ITERATIONS_TOTAL.with_label_values(&[database, "error"]);
    for error_type in ERROR_TYPES {
        DB_ERRORS.with_label_values(&[database, error_type]);
    }
    TABLE_RECREATED.with_label_values(&[database]);
    RW_ROW_CONTENTION.with_label_values(&[database]);
    for operation in TABLE_MAINTENANCE_OPERATIONS {
        TABLE_MAINTENANCE_ERRORS.with_label_values(&[database, operation]);
    }
    DB_READONLY.with_label_values(&[database]).set(0);
    LAST_SUCCESS.with_label_values(&[database]).set(0);
    LAST_RUNTIME_MS.with_label_values(&[database]).set(0);
}

/// Encode and return metrics for HTTP export
///
/// # Errors
///
/// Returns an error if metrics encoding fails
pub fn encode_metrics() -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    let encoder = prometheus::TextEncoder::new();

    encoder
        .encode(&REGISTRY.gather(), &mut buffer)
        .map_err(|e| format!("could not encode custom metrics: {e}"))?;

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;

    #[test]
    fn test_metrics_initialization() {
        // Test that all metrics can be accessed without panicking
        PULSE.set(1);
        assert_eq!(PULSE.get(), 1);
    }

    #[test]
    fn test_metrics_labels() {
        // Test metrics with labels
        DB_ERRORS
            .with_label_values(&["postgres", "connection"])
            .inc();
        OPERATION_DURATION
            .with_label_values(&["postgres", "connect"])
            .observe(0.123);
        ROWS_AFFECTED
            .with_label_values(&["mysql", "insert"])
            .inc_by(5);
        ITERATIONS_TOTAL
            .with_label_values(&["postgres", "success"])
            .inc();
        LAST_SUCCESS
            .with_label_values(&["postgres"])
            .set(1_234_567_890);
        TABLE_SIZE_BYTES
            .with_label_values(&["postgres", "dbpulse_rw"])
            .set(1024);
        TABLE_ROWS
            .with_label_values(&["mysql", "dbpulse_rw"])
            .set(100);
        DB_READONLY.with_label_values(&["postgres"]).set(0);
        TLS_HANDSHAKE_DURATION
            .with_label_values(&["postgres"])
            .observe(0.05);
        TLS_CONNECTION_ERRORS
            .with_label_values(&["mysql", "handshake"])
            .inc();
        TLS_INFO
            .with_label_values(&["postgres", "TLSv1.3", "AES256-GCM-SHA384"])
            .set(1);
        TLS_CERT_EXPIRY_DAYS
            .with_label_values(&["postgres"])
            .set(90);
        TLS_CERT_PROBE_ERRORS
            .with_label_values(&["postgres", "handshake"])
            .inc();
        DATABASE_HOST_INFO
            .with_label_values(&["mysql", "db-node-a"])
            .set(1);
    }

    #[test]
    fn test_histogram_metrics() {
        // Test histogram observations
        RUNTIME.start_timer().observe_duration();
        CONNECTION_DURATION.observe(1.5);
        OPERATION_DURATION
            .with_label_values(&["postgres", "insert"])
            .observe(0.001);
        TLS_HANDSHAKE_DURATION
            .with_label_values(&["mysql"])
            .observe(0.1);
    }

    #[test]
    fn test_counter_metrics() {
        // Test counters
        PANICS_RECOVERED.inc();
        DB_ERRORS.with_label_values(&["postgres", "timeout"]).inc();
        ROWS_AFFECTED
            .with_label_values(&["postgres", "delete"])
            .inc_by(10);
        TLS_CONNECTION_ERRORS
            .with_label_values(&["postgres", "certificate"])
            .inc();
    }

    #[test]
    fn test_encode_metrics() {
        // Initialize at least one metric to ensure non-empty output
        PANICS_RECOVERED.inc();

        // Ensure metrics can be encoded without error
        let result = encode_metrics();
        assert!(result.is_ok());

        let buffer = result.unwrap();
        assert!(!buffer.is_empty());

        // Verify it's valid UTF-8 and contains some expected metric names
        let output = String::from_utf8(buffer).unwrap();
        assert!(output.contains("dbpulse"));
    }

    #[test]
    fn test_registry() {
        // Force initialization of metrics by accessing them
        let _ = &*PULSE;
        let _ = &*RUNTIME;
        // Use metrics with labels to ensure they're registered
        DB_ERRORS.with_label_values(&["test", "test"]).inc();
        OPERATION_DURATION
            .with_label_values(&["test", "test"])
            .observe(0.1);
        DATABASE_HOST_INFO
            .with_label_values(&["test", "db-1"])
            .set(1);

        // Test that registry can gather metrics
        let metrics = REGISTRY.gather();
        assert!(!metrics.is_empty());

        // Check that our custom metrics are registered
        let metric_names: Vec<String> = metrics.iter().map(|m| m.name().to_string()).collect();

        // Check for some expected metrics
        assert!(metric_names.contains(&"dbpulse_pulse".to_string()));
        assert!(metric_names.contains(&"dbpulse_runtime".to_string()));
        assert!(metric_names.contains(&"dbpulse_errors_total".to_string()));
        assert!(metric_names.contains(&"dbpulse_operation_duration_seconds".to_string()));
        assert!(metric_names.contains(&"dbpulse_database_host_info".to_string()));
    }

    #[test]
    fn test_gauge_operations() {
        // Test gauge set/get operations
        PULSE.set(0);
        assert_eq!(PULSE.get(), 0);
        PULSE.set(1);
        assert_eq!(PULSE.get(), 1);
    }

    #[test]
    fn test_all_error_types() {
        // Test all error classification types
        let error_types = [
            "authentication",
            "timeout",
            "connection",
            "transaction",
            "query",
        ];

        for error_type in &error_types {
            DB_ERRORS.with_label_values(&["postgres", error_type]).inc();
            DB_ERRORS.with_label_values(&["mysql", error_type]).inc();
        }
    }

    #[test]
    fn test_all_operations() {
        // Test all operation types
        let operations = [
            "connect",
            "create_table",
            "insert",
            "select",
            "transaction_test",
            "cleanup",
        ];

        for op in &operations {
            OPERATION_DURATION
                .with_label_values(&["postgres", op])
                .observe(0.01);
            OPERATION_DURATION
                .with_label_values(&["mysql", op])
                .observe(0.01);
        }
    }
}
