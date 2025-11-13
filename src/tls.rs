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
            _ => Err(format!("Invalid TLS mode: {}", s)),
        }
    }
}

impl TlsMode {
    /// Check if TLS is enabled
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
}
