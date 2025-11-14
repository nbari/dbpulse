pub mod mysql;
pub mod postgres;

use crate::tls::TlsMetadata;

/// Result from database health check including TLS metadata
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// Database version string
    pub version: String,
    /// TLS metadata (if TLS is enabled)
    pub tls_metadata: Option<TlsMetadata>,
}
