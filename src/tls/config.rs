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

    /// Accepts both the PostgreSQL spellings and the MySQL/MariaDB ones, since
    /// the DSN advertises `ssl-mode` as an alias of `sslmode` and a MySQL user
    /// will reasonably write `REQUIRED` or `VERIFY_IDENTITY`.
    ///
    /// Anything unrecognised is an error rather than a silent fallback: this
    /// value decides whether credentials cross the network in plaintext, so it
    /// must fail closed.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().replace('_', "-").as_str() {
            "disable" | "disabled" => Ok(Self::Disable),
            "require" | "required" => Ok(Self::Require),
            "verify-ca" => Ok(Self::VerifyCA),
            "verify-full" | "verify-identity" => Ok(Self::VerifyFull),
            // Opportunistic modes have no equivalent here: dbpulse either
            // requires TLS or does not use it, and silently picking one of
            // those for the user is how a monitor ends up in plaintext.
            other @ ("prefer" | "preferred" | "allow") => Err(format!(
                "unsupported TLS mode `{other}`: dbpulse does not negotiate opportunistic TLS, \
                 use `disable` or `require`"
            )),
            _ => Err(format!(
                "invalid TLS mode `{s}`, expected one of: disable, require, verify-ca, verify-full \
                 (MySQL spellings DISABLED, REQUIRED, VERIFY_CA, VERIFY_IDENTITY are also accepted)"
            )),
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
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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

    /// Regression: an unrecognised mode must be an error, never a silent
    /// fallback.
    ///
    /// `extract_tls_config` used `.ok().unwrap_or_default()`, and the default
    /// is `Disable`, so any typo downgraded the connection to plaintext and
    /// shipped the credentials in the clear without a word of warning.
    #[test]
    fn unknown_tls_mode_is_rejected_rather_than_downgraded() {
        for input in ["verify_full_typo", "verify-al", "on", "true", "yes", ""] {
            let parsed = input.parse::<TlsMode>();
            assert!(
                parsed.is_err(),
                "`{input}` must be rejected, not silently treated as {:?}",
                TlsMode::default()
            );
        }
    }

    /// The DSN documents `ssl-mode` as an alias of `sslmode`, so a MySQL user
    /// will write MySQL's values. Every one of these previously failed to parse
    /// and therefore fell back to plaintext.
    #[test]
    fn mysql_spellings_are_accepted() {
        assert_eq!("DISABLED".parse::<TlsMode>().unwrap(), TlsMode::Disable);
        assert_eq!("REQUIRED".parse::<TlsMode>().unwrap(), TlsMode::Require);
        assert_eq!("VERIFY_CA".parse::<TlsMode>().unwrap(), TlsMode::VerifyCA);
        assert_eq!(
            "VERIFY_IDENTITY".parse::<TlsMode>().unwrap(),
            TlsMode::VerifyFull
        );
    }

    /// Opportunistic modes must not be guessed at in either direction:
    /// mapping them to `require` can break a working deployment, and mapping
    /// them to `disable` is the plaintext downgrade all over again.
    #[test]
    fn opportunistic_modes_are_rejected_with_guidance() {
        for input in ["prefer", "preferred", "allow"] {
            let err = input
                .parse::<TlsMode>()
                .expect_err("opportunistic modes have no equivalent");
            assert!(
                err.contains("disable") && err.contains("require"),
                "error should point at the supported modes, got: {err}"
            );
        }
    }

    #[test]
    fn test_tls_mode_is_enabled() {
        assert!(!TlsMode::Disable.is_enabled());
        assert!(TlsMode::Require.is_enabled());
        assert!(TlsMode::VerifyCA.is_enabled());
        assert!(TlsMode::VerifyFull.is_enabled());
    }
}
