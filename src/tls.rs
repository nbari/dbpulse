use std::path::PathBuf;
use std::str::FromStr;

/// TLS configuration for database connections
#[derive(Debug, Clone, Default)]
pub struct TlsConfig {
    pub mode: TlsMode,
    pub ca: Option<PathBuf>,
    pub cert: Option<PathBuf>,
    pub key: Option<PathBuf>,
}

/// TLS/SSL mode for database connections
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TlsMode {
    /// No TLS encryption
    #[default]
    Disable,
    /// TLS required, but no certificate verification
    Require,
    /// Verify server certificate against CA
    VerifyCA,
    /// Verify certificate and hostname
    VerifyFull,
}

impl FromStr for TlsMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "disable" => Ok(Self::Disable),
            "require" => Ok(Self::Require),
            "verify-ca" => Ok(Self::VerifyCA),
            "verify-full" => Ok(Self::VerifyFull),
            _ => Err(format!("Invalid TLS mode: {s}")),
        }
    }
}

impl TlsMode {
    /// Check if TLS is enabled
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disable)
    }
}

/// TLS connection metadata extracted after handshake
#[derive(Debug, Clone, Default)]
pub struct TlsMetadata {
    /// TLS version (e.g., "TLSv1.2", "TLSv1.3")
    pub version: Option<String>,
    /// Cipher suite in use
    pub cipher: Option<String>,
    /// Certificate subject (if available)
    pub cert_subject: Option<String>,
    /// Certificate issuer (if available)
    pub cert_issuer: Option<String>,
    /// Days until certificate expiration
    pub cert_expiry_days: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tls_mode_from_str() {
        assert_eq!("disable".parse::<TlsMode>().unwrap(), TlsMode::Disable);
        assert_eq!("require".parse::<TlsMode>().unwrap(), TlsMode::Require);
        assert_eq!("verify-ca".parse::<TlsMode>().unwrap(), TlsMode::VerifyCA);
        assert_eq!(
            "verify-full".parse::<TlsMode>().unwrap(),
            TlsMode::VerifyFull
        );
        assert_eq!("DISABLE".parse::<TlsMode>().unwrap(), TlsMode::Disable);
        assert!("invalid".parse::<TlsMode>().is_err());
    }

    #[test]
    fn test_tls_mode_is_enabled() {
        assert!(!TlsMode::Disable.is_enabled());
        assert!(TlsMode::Require.is_enabled());
        assert!(TlsMode::VerifyCA.is_enabled());
        assert!(TlsMode::VerifyFull.is_enabled());
    }

    #[test]
    fn test_tls_mode_from_str_all_cases() {
        // Test all lowercase
        assert_eq!("disable".parse::<TlsMode>().unwrap(), TlsMode::Disable);
        assert_eq!("require".parse::<TlsMode>().unwrap(), TlsMode::Require);
        assert_eq!("verify-ca".parse::<TlsMode>().unwrap(), TlsMode::VerifyCA);
        assert_eq!(
            "verify-full".parse::<TlsMode>().unwrap(),
            TlsMode::VerifyFull
        );

        // Test uppercase
        assert_eq!("DISABLE".parse::<TlsMode>().unwrap(), TlsMode::Disable);
        assert_eq!("REQUIRE".parse::<TlsMode>().unwrap(), TlsMode::Require);
        assert_eq!("VERIFY-CA".parse::<TlsMode>().unwrap(), TlsMode::VerifyCA);
        assert_eq!(
            "VERIFY-FULL".parse::<TlsMode>().unwrap(),
            TlsMode::VerifyFull
        );

        // Test mixed case
        assert_eq!("Disable".parse::<TlsMode>().unwrap(), TlsMode::Disable);
        assert_eq!("Require".parse::<TlsMode>().unwrap(), TlsMode::Require);
        assert_eq!("Verify-CA".parse::<TlsMode>().unwrap(), TlsMode::VerifyCA);
        assert_eq!(
            "Verify-Full".parse::<TlsMode>().unwrap(),
            TlsMode::VerifyFull
        );
    }

    #[test]
    fn test_tls_mode_from_str_invalid() {
        assert!("invalid".parse::<TlsMode>().is_err());
        assert!("".parse::<TlsMode>().is_err());
        assert!("enabled".parse::<TlsMode>().is_err());
        assert!("verify".parse::<TlsMode>().is_err());
        assert!("verify-identity".parse::<TlsMode>().is_err());
        assert!("ssl".parse::<TlsMode>().is_err());

        let err = "unknown".parse::<TlsMode>().unwrap_err();
        assert!(err.contains("Invalid TLS mode"));
        assert!(err.contains("unknown"));
    }

    #[test]
    fn test_tls_mode_default() {
        let mode = TlsMode::default();
        assert_eq!(mode, TlsMode::Disable);
        assert!(!mode.is_enabled());
    }

    #[test]
    fn test_tls_mode_debug() {
        let debug_str = format!("{:?}", TlsMode::Disable);
        assert_eq!(debug_str, "Disable");

        let debug_str = format!("{:?}", TlsMode::Require);
        assert_eq!(debug_str, "Require");

        let debug_str = format!("{:?}", TlsMode::VerifyCA);
        assert_eq!(debug_str, "VerifyCA");

        let debug_str = format!("{:?}", TlsMode::VerifyFull);
        assert_eq!(debug_str, "VerifyFull");
    }

    #[test]
    fn test_tls_mode_clone() {
        let mode = TlsMode::Require;
        let cloned = mode;
        assert_eq!(mode, cloned);
    }

    #[test]
    fn test_tls_mode_copy() {
        let mode = TlsMode::Require;
        let copied = mode;
        assert_eq!(mode, copied);
    }

    #[test]
    fn test_tls_mode_equality() {
        assert_eq!(TlsMode::Disable, TlsMode::Disable);
        assert_eq!(TlsMode::Require, TlsMode::Require);
        assert_eq!(TlsMode::VerifyCA, TlsMode::VerifyCA);
        assert_eq!(TlsMode::VerifyFull, TlsMode::VerifyFull);

        assert_ne!(TlsMode::Disable, TlsMode::Require);
        assert_ne!(TlsMode::Require, TlsMode::VerifyCA);
        assert_ne!(TlsMode::VerifyCA, TlsMode::VerifyFull);
    }

    #[test]
    fn test_tls_config_default() {
        let config = TlsConfig::default();
        assert_eq!(config.mode, TlsMode::Disable);
        assert!(config.ca.is_none());
        assert!(config.cert.is_none());
        assert!(config.key.is_none());
    }

    #[test]
    fn test_tls_config_debug() {
        let config = TlsConfig::default();
        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("TlsConfig"));
        assert!(debug_str.contains("mode"));
    }

    #[test]
    fn test_tls_config_clone() {
        let config = TlsConfig {
            mode: TlsMode::Require,
            ca: Some(PathBuf::from("/path/to/ca.crt")),
            cert: Some(PathBuf::from("/path/to/cert.crt")),
            key: Some(PathBuf::from("/path/to/key.pem")),
        };

        let cloned = config.clone();
        assert_eq!(cloned.mode, config.mode);
        assert_eq!(cloned.ca, config.ca);
        assert_eq!(cloned.cert, config.cert);
        assert_eq!(cloned.key, config.key);
    }

    #[test]
    fn test_tls_config_with_paths() {
        let config = TlsConfig {
            mode: TlsMode::VerifyFull,
            ca: Some(PathBuf::from("/etc/ssl/ca.crt")),
            cert: Some(PathBuf::from("/etc/ssl/client.crt")),
            key: Some(PathBuf::from("/etc/ssl/client.key")),
        };

        assert_eq!(config.mode, TlsMode::VerifyFull);
        assert_eq!(
            config.ca.as_ref().unwrap(),
            &PathBuf::from("/etc/ssl/ca.crt")
        );
        assert_eq!(
            config.cert.as_ref().unwrap(),
            &PathBuf::from("/etc/ssl/client.crt")
        );
        assert_eq!(
            config.key.as_ref().unwrap(),
            &PathBuf::from("/etc/ssl/client.key")
        );
    }

    #[test]
    fn test_tls_metadata_default() {
        let metadata = TlsMetadata::default();
        assert!(metadata.version.is_none());
        assert!(metadata.cipher.is_none());
        assert!(metadata.cert_subject.is_none());
        assert!(metadata.cert_issuer.is_none());
        assert!(metadata.cert_expiry_days.is_none());
    }

    #[test]
    fn test_tls_metadata_debug() {
        let metadata = TlsMetadata::default();
        let debug_str = format!("{metadata:?}");
        assert!(debug_str.contains("TlsMetadata"));
    }

    #[test]
    fn test_tls_metadata_clone() {
        let metadata = TlsMetadata {
            version: Some("TLSv1.3".to_string()),
            cipher: Some("AES256-GCM-SHA384".to_string()),
            cert_subject: Some("CN=example.com".to_string()),
            cert_issuer: Some("CN=CA".to_string()),
            cert_expiry_days: Some(90),
        };

        let cloned = metadata.clone();
        assert_eq!(cloned.version, metadata.version);
        assert_eq!(cloned.cipher, metadata.cipher);
        assert_eq!(cloned.cert_subject, metadata.cert_subject);
        assert_eq!(cloned.cert_issuer, metadata.cert_issuer);
        assert_eq!(cloned.cert_expiry_days, metadata.cert_expiry_days);
    }

    #[test]
    fn test_tls_metadata_with_values() {
        let metadata = TlsMetadata {
            version: Some("TLSv1.2".to_string()),
            cipher: Some("ECDHE-RSA-AES128-GCM-SHA256".to_string()),
            cert_subject: Some("CN=db.example.com,O=Example Corp".to_string()),
            cert_issuer: Some("CN=Example CA,O=Example Corp".to_string()),
            cert_expiry_days: Some(365),
        };

        assert_eq!(metadata.version.as_ref().unwrap(), "TLSv1.2");
        assert_eq!(
            metadata.cipher.as_ref().unwrap(),
            "ECDHE-RSA-AES128-GCM-SHA256"
        );
        assert_eq!(
            metadata.cert_subject.as_ref().unwrap(),
            "CN=db.example.com,O=Example Corp"
        );
        assert_eq!(
            metadata.cert_issuer.as_ref().unwrap(),
            "CN=Example CA,O=Example Corp"
        );
        assert_eq!(metadata.cert_expiry_days.unwrap(), 365);
    }

    #[test]
    fn test_tls_metadata_partial() {
        let metadata = TlsMetadata {
            version: Some("TLSv1.3".to_string()),
            cipher: Some("AES256-GCM-SHA384".to_string()),
            cert_subject: None,
            cert_issuer: None,
            cert_expiry_days: None,
        };

        assert!(metadata.version.is_some());
        assert!(metadata.cipher.is_some());
        assert!(metadata.cert_subject.is_none());
        assert!(metadata.cert_issuer.is_none());
        assert!(metadata.cert_expiry_days.is_none());
    }

    #[test]
    fn test_tls_metadata_negative_expiry() {
        let metadata = TlsMetadata {
            version: Some("TLSv1.3".to_string()),
            cipher: None,
            cert_subject: None,
            cert_issuer: None,
            cert_expiry_days: Some(-10), // Expired certificate
        };

        assert_eq!(metadata.cert_expiry_days.unwrap(), -10);
    }

    #[test]
    fn test_tls_metadata_full_certificate_info() {
        // Test with complete certificate metadata
        let metadata = TlsMetadata {
            version: Some("TLSv1.3".to_string()),
            cipher: Some("AES256-GCM-SHA384".to_string()),
            cert_subject: Some("CN=db.example.com,O=Example Corp,C=US".to_string()),
            cert_issuer: Some("CN=Example CA,O=Example Corp,C=US".to_string()),
            cert_expiry_days: Some(90),
        };

        assert_eq!(metadata.version.as_ref().unwrap(), "TLSv1.3");
        assert_eq!(
            metadata.cipher.as_ref().unwrap(),
            "AES256-GCM-SHA384"
        );
        assert_eq!(
            metadata.cert_subject.as_ref().unwrap(),
            "CN=db.example.com,O=Example Corp,C=US"
        );
        assert_eq!(
            metadata.cert_issuer.as_ref().unwrap(),
            "CN=Example CA,O=Example Corp,C=US"
        );
        assert_eq!(metadata.cert_expiry_days.unwrap(), 90);
    }

    #[test]
    fn test_tls_metadata_expiry_warnings() {
        // Test various expiry warning thresholds
        let test_cases = vec![
            (90, "healthy certificate"),
            (30, "approaching expiry"),
            (7, "critical - renew soon"),
            (1, "expires tomorrow"),
            (0, "expires today"),
            (-1, "expired yesterday"),
            (-30, "expired 30 days ago"),
        ];

        for (days, description) in test_cases {
            let metadata = TlsMetadata {
                version: Some("TLSv1.3".to_string()),
                cipher: Some("AES256-GCM-SHA384".to_string()),
                cert_subject: Some("CN=test.db".to_string()),
                cert_issuer: Some("CN=Test CA".to_string()),
                cert_expiry_days: Some(days),
            };

            assert_eq!(
                metadata.cert_expiry_days.unwrap(),
                days,
                "Failed for: {}",
                description
            );
        }
    }

    #[test]
    fn test_tls_metadata_mysql_format() {
        // Test metadata format typical from MySQL SHOW STATUS
        let metadata = TlsMetadata {
            version: Some("TLSv1.2".to_string()),
            cipher: Some("DHE-RSA-AES256-SHA".to_string()),
            cert_subject: Some("/C=US/ST=California/L=San Francisco/O=Example/CN=mysql.example.com".to_string()),
            cert_issuer: Some("/C=US/O=DigiCert Inc/CN=DigiCert Global Root CA".to_string()),
            cert_expiry_days: Some(365),
        };

        // Verify all fields are populated
        assert!(metadata.version.is_some());
        assert!(metadata.cipher.is_some());
        assert!(metadata.cert_subject.is_some());
        assert!(metadata.cert_issuer.is_some());
        assert!(metadata.cert_expiry_days.is_some());

        // Verify subject contains expected fields
        let subject = metadata.cert_subject.unwrap();
        assert!(subject.contains("CN=mysql.example.com"));
        assert!(subject.contains("C=US"));
    }

    #[test]
    fn test_tls_metadata_expiry_only() {
        // Test metadata with only expiry information
        let metadata = TlsMetadata {
            version: None,
            cipher: None,
            cert_subject: None,
            cert_issuer: None,
            cert_expiry_days: Some(45),
        };

        assert!(metadata.version.is_none());
        assert!(metadata.cipher.is_none());
        assert!(metadata.cert_subject.is_none());
        assert!(metadata.cert_issuer.is_none());
        assert_eq!(metadata.cert_expiry_days.unwrap(), 45);
    }
}
