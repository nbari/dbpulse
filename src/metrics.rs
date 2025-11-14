use lazy_static::lazy_static;
use prometheus::{
    Encoder, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    IntGaugeVec, Registry, opts, register_histogram_vec_with_registry,
    register_histogram_with_registry, register_int_counter_vec_with_registry,
    register_int_counter_with_registry, register_int_gauge_vec_with_registry,
    register_int_gauge_with_registry,
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref PULSE: IntGauge = register_int_gauge_with_registry!(
        "dbpuse_pulse",
        "1 ok, 0 error",
        REGISTRY
    )
    .expect("metric can be created");
    pub static ref RUNTIME: Histogram = register_histogram_with_registry!(
        HistogramOpts::new("dbpulse_runtime", "pulse latency in seconds"),
        REGISTRY
    )
    .expect("metric can be created");

    // TLS-specific metrics
    pub static ref TLS_HANDSHAKE_DURATION: HistogramVec = register_histogram_vec_with_registry!(
        HistogramOpts::new(
            "dbpulse_tls_handshake_duration_seconds",
            "TLS handshake duration in seconds"
        ),
        &["database"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref TLS_CONNECTION_ERRORS: IntCounterVec = register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_tls_connection_errors_total",
            "Total TLS connection errors by type"
        ),
        &["database", "error_type"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref TLS_INFO: IntGaugeVec = register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_tls_info",
            "TLS connection info (version, cipher) - value is always 1"
        ),
        &["database", "version", "cipher"],
        REGISTRY
    )
    .expect("metric can be created");

    // Critical Priority Metrics
    pub static ref DB_ERRORS: IntCounterVec = register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_errors_total",
            "Total database errors by type"
        ),
        &["database", "error_type"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref OPERATION_DURATION: HistogramVec = register_histogram_vec_with_registry!(
        HistogramOpts::new(
            "dbpulse_operation_duration_seconds",
            "Duration of specific database operations"
        ),
        &["database", "operation"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref CONNECTION_DURATION: Histogram = register_histogram_with_registry!(
        HistogramOpts::new(
            "dbpulse_connection_duration_seconds",
            "Time connection is held open"
        ),
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref CONNECTIONS_ACTIVE: IntGauge = register_int_gauge_with_registry!(
        "dbpulse_connections_active",
        "Currently active database connections",
        REGISTRY
    )
    .expect("metric can be created");

    // High Priority Metrics
    pub static ref ROWS_AFFECTED: IntCounterVec = register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_rows_affected_total",
            "Total rows affected by operations"
        ),
        &["database", "operation"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref ITERATIONS_TOTAL: IntCounterVec = register_int_counter_vec_with_registry!(
        opts!(
            "dbpulse_iterations_total",
            "Total monitoring iterations"
        ),
        &["database", "status"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref LAST_SUCCESS: IntGaugeVec = register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_last_success_timestamp_seconds",
            "Unix timestamp of last successful check"
        ),
        &["database"],
        REGISTRY
    )
    .expect("metric can be created");

    // Medium Priority Metrics
    pub static ref TABLE_SIZE_BYTES: IntGaugeVec = register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_table_size_bytes",
            "Approximate table size in bytes"
        ),
        &["database", "table"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref TABLE_ROWS: IntGaugeVec = register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_table_rows",
            "Approximate row count"
        ),
        &["database", "table"],
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref PANICS_RECOVERED: IntCounter = register_int_counter_with_registry!(
        opts!(
            "dbpulse_panics_recovered_total",
            "Total panics recovered from"
        ),
        REGISTRY
    )
    .expect("metric can be created");

    pub static ref DB_READONLY: IntGaugeVec = register_int_gauge_vec_with_registry!(
        opts!(
            "dbpulse_database_readonly",
            "1 if database is in read-only mode"
        ),
        &["database"],
        REGISTRY
    )
    .expect("metric can be created");
}

/// Encode and return metrics for HTTP export
pub fn encode_metrics() -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    let encoder = prometheus::TextEncoder::new();

    encoder
        .encode(&REGISTRY.gather(), &mut buffer)
        .map_err(|e| format!("could not encode custom metrics: {}", e))?;

    Ok(buffer)
}
