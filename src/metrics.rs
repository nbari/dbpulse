use std::sync::LazyLock;

use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Registry, opts, register_histogram_vec_with_registry,
    register_histogram_with_registry, register_int_counter_vec_with_registry,
    register_int_counter_with_registry, register_int_gauge_vec_with_registry,
    register_int_gauge_with_registry,
};

pub static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

pub static PULSE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge_with_registry!("dbpuse_pulse", "1 ok, 0 error", &REGISTRY)
        .expect("metric can be created")
});

pub static RUNTIME: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram_with_registry!(
        HistogramOpts::new("dbpulse_runtime", "pulse latency in seconds"),
        &REGISTRY
    )
    .expect("metric can be created")
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
    .expect("metric can be created")
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
    .expect("metric can be created")
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
    .expect("metric can be created")
});

// Critical Priority Metrics
pub static DB_ERRORS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!("dbpulse_errors_total", "Total database errors by type"),
        &["database", "error_type"],
        &REGISTRY
    )
    .expect("metric can be created")
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
    .expect("metric can be created")
});

pub static CONNECTION_DURATION: LazyLock<Histogram> = LazyLock::new(|| {
    register_histogram_with_registry!(
        HistogramOpts::new(
            "dbpulse_connection_duration_seconds",
            "Time connection is held open"
        ),
        &REGISTRY
    )
    .expect("metric can be created")
});

pub static CONNECTIONS_ACTIVE: LazyLock<IntGauge> = LazyLock::new(|| {
    register_int_gauge_with_registry!(
        "dbpulse_connections_active",
        "Currently active database connections",
        &REGISTRY
    )
    .expect("metric can be created")
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
    .expect("metric can be created")
});

pub static ITERATIONS_TOTAL: LazyLock<IntCounterVec> = LazyLock::new(|| {
    register_int_counter_vec_with_registry!(
        opts!("dbpulse_iterations_total", "Total monitoring iterations"),
        &["database", "status"],
        &REGISTRY
    )
    .expect("metric can be created")
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
    .expect("metric can be created")
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
    .expect("metric can be created")
});

pub static TABLE_ROWS: LazyLock<IntGaugeVec> = LazyLock::new(|| {
    register_int_gauge_vec_with_registry!(
        opts!("dbpulse_table_rows", "Approximate row count"),
        &["database", "table"],
        &REGISTRY
    )
    .expect("metric can be created")
});

pub static PANICS_RECOVERED: LazyLock<IntCounter> = LazyLock::new(|| {
    register_int_counter_with_registry!(
        opts!(
            "dbpulse_panics_recovered_total",
            "Total panics recovered from"
        ),
        &REGISTRY
    )
    .expect("metric can be created")
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
    .expect("metric can be created")
});

/// Encode and return metrics for HTTP export
pub fn encode_metrics() -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    let encoder = prometheus::TextEncoder::new();

    encoder
        .encode(&REGISTRY.gather(), &mut buffer)
        .map_err(|e| format!("could not encode custom metrics: {}", e))?;

    Ok(buffer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_initialization() {
        // Test that all metrics can be accessed without panicking
        PULSE.set(1);
        assert_eq!(PULSE.get(), 1);

        // Test inc/dec without checking exact values (shared state across tests)
        let before = CONNECTIONS_ACTIVE.get();
        CONNECTIONS_ACTIVE.inc();
        assert_eq!(CONNECTIONS_ACTIVE.get(), before + 1);
        CONNECTIONS_ACTIVE.dec();
        assert_eq!(CONNECTIONS_ACTIVE.get(), before);
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
            .set(1234567890);
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
        let _ = &*DB_ERRORS;
        let _ = &*OPERATION_DURATION;

        // Test that registry can gather metrics
        let metrics = REGISTRY.gather();
        assert!(!metrics.is_empty());

        // Check that our custom metrics are registered
        let metric_names: Vec<String> = metrics.iter().map(|m| m.name().to_string()).collect();

        // Check for some expected metrics (note: PULSE has typo "dbpuse_pulse")
        assert!(metric_names.contains(&"dbpuse_pulse".to_string()));
        assert!(metric_names.contains(&"dbpulse_runtime".to_string()));
        assert!(metric_names.contains(&"dbpulse_errors_total".to_string()));
        assert!(metric_names.contains(&"dbpulse_operation_duration_seconds".to_string()));
    }

    #[test]
    fn test_gauge_operations() {
        // Test gauge set/get operations
        PULSE.set(0);
        assert_eq!(PULSE.get(), 0);
        PULSE.set(1);
        assert_eq!(PULSE.get(), 1);

        CONNECTIONS_ACTIVE.set(5);
        assert_eq!(CONNECTIONS_ACTIVE.get(), 5);
        CONNECTIONS_ACTIVE.inc();
        assert_eq!(CONNECTIONS_ACTIVE.get(), 6);
        CONNECTIONS_ACTIVE.dec();
        assert_eq!(CONNECTIONS_ACTIVE.get(), 5);
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
