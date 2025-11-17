use std::{path::PathBuf, str::FromStr};

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
    }

    #[test]
    fn test_tls_mode_case_insensitive() {
        assert_eq!("DISABLE".parse::<TlsMode>().unwrap(), TlsMode::Disable);
        assert_eq!("Require".parse::<TlsMode>().unwrap(), TlsMode::Require);
    }

    #[test]
    fn test_tls_mode_is_enabled() {
        assert!(!TlsMode::Disable.is_enabled());
        assert!(TlsMode::Require.is_enabled());
        assert!(TlsMode::VerifyCA.is_enabled());
        assert!(TlsMode::VerifyFull.is_enabled());
    }
}
