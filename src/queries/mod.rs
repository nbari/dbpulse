pub mod mysql;
pub mod postgres;

use crate::tls::TlsMetadata;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_result_without_tls() {
        let result = HealthCheckResult {
            version: "PostgreSQL 15.0".to_string(),
            db_host: Some("db-1".to_string()),
            uptime_seconds: Some(1_000),
            tls_metadata: None,
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
            version: "PostgreSQL 13.0 in recovery mode".to_string(),
            db_host: Some("replica-1".to_string()),
            uptime_seconds: Some(900),
            tls_metadata: Some(tls_metadata),
        };

        assert!(result.version.contains("recovery mode"));
        assert_eq!(result.db_host, Some("replica-1".to_string()));
        let tls = result.tls_metadata.as_ref().unwrap();
        assert_eq!(tls.cert_subject.as_ref().unwrap(), "CN=db.example.com");
        assert_eq!(tls.cert_issuer.as_ref().unwrap(), "CN=Example CA");
        assert_eq!(tls.cert_expiry_days.unwrap(), 90);
    }

    #[test]
    fn test_health_check_result_mysql_read_only() {
        let result = HealthCheckResult {
            version: "MySQL 8.0.30 read-only".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
        };

        assert!(result.version.contains("read-only"));
    }

    #[test]
    fn test_health_check_result_version_with_special_chars() {
        let result = HealthCheckResult {
            version: "PostgreSQL 15.0 (Ubuntu 15.0-1.pgdg22.04+1)".to_string(),
            db_host: None,
            uptime_seconds: None,
            tls_metadata: None,
        };

        assert!(result.version.contains("Ubuntu"));
        assert!(result.version.contains("pgdg22.04"));
    }
}
