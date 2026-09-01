use crate::{
    cli::actions::Action,
    tls::{TlsConfig, TlsMode},
};
use anyhow::{Context, Result};
use clap::ArgMatches;
use dsn::DSN;
use std::{net::IpAddr, path::PathBuf};

/// Look up a DSN query parameter by alias, case-insensitively.
///
/// The `dsn` crate stores parameter keys verbatim, so `?SSLMODE=require` does
/// not match a lookup for `sslmode`. Key case must not decide security
/// posture: missing `SSLMODE` would silently default to `TlsMode::Disable`
/// and ship the credentials in plaintext -- the same fail-open an
/// unrecognised value used to cause.
fn get_param<'a>(dsn: &'a DSN, aliases: &[&str]) -> Option<&'a str> {
    aliases.iter().find_map(|alias| {
        dsn.params
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(alias))
            .map(|(_, value)| value.as_str())
    })
}

/// Extract TLS configuration from DSN query parameters
///
/// Supports both PostgreSQL-style and MySQL-style parameter names:
/// - sslmode, ssl-mode: disable|require|verify-ca|verify-full
/// - sslrootcert, sslca, ssl-ca: Path to CA certificate
/// - sslcert, ssl-cert: Path to client certificate
/// - sslkey, ssl-key: Path to client private key
fn extract_tls_config(dsn: &DSN) -> Result<TlsConfig> {
    // Extract TLS mode (PostgreSQL and MySQL spellings)
    let mode = get_param(dsn, &["sslmode", "ssl-mode"])
        .map(|m| m.parse::<TlsMode>().map_err(|err| anyhow::anyhow!(err)))
        .transpose()
        .context("invalid TLS mode in DSN")?
        .unwrap_or_default();

    // Extract CA certificate path (try multiple parameter names)
    let ca = get_param(dsn, &["sslrootcert", "sslca", "ssl-ca"]).map(PathBuf::from);

    // Extract client certificate path
    let cert = get_param(dsn, &["sslcert", "ssl-cert"]).map(PathBuf::from);

    // Extract client key path
    let key = get_param(dsn, &["sslkey", "ssl-key"]).map(PathBuf::from);

    Ok(TlsConfig {
        mode,
        ca,
        cert,
        key,
    })
}

/// Convert `ArgMatches` into typed Action enum with validation
///
/// # Errors
///
/// Returns an error if the DSN is invalid or required parameters are missing
pub fn dispatch(matches: &ArgMatches) -> Result<Action> {
    // Extract DSN
    let dsn_str = matches
        .get_one::<String>("dsn")
        .context("DSN is required")?;
    let dsn = dsn::parse(dsn_str).context("Failed to parse DSN")?;

    // Extract interval with default
    let interval = matches.get_one::<u16>("interval").copied().unwrap_or(30);

    // Extract and validate listen address
    let listen = matches
        .get_one::<String>("listen")
        .map(|addr| {
            addr.parse::<IpAddr>()
                .with_context(|| format!("Invalid IP address: {addr}"))
        })
        .transpose()?;

    // Extract port with default
    let port = matches.get_one::<u16>("port").copied().unwrap_or(9300);

    // Extract range with default
    let range = matches.get_one::<u32>("range").copied().unwrap_or(100);

    // Extract TLS configuration from DSN query parameters
    let tls = extract_tls_config(&dsn)?;

    Ok(Action::Monitor {
        dsn,
        interval,
        listen,
        port,
        range,
        tls,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

    use super::*;
    use crate::cli::commands;

    #[test]
    fn test_dispatch_valid_mysql() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec!["dbpulse", "--dsn", "mysql://user:pass@localhost/db"])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval,
                listen,
                port,
                range,
                tls,
            } => {
                assert_eq!(dsn.driver, "mysql");
                assert_eq!(interval, 30);
                assert_eq!(listen, None);
                assert_eq!(port, 9300);
                assert_eq!(range, 100);
                assert_eq!(tls.mode, TlsMode::Disable);
            }
        }
    }

    #[test]
    fn test_dispatch_valid_postgres() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "postgres://user:pass@localhost/db",
                "--interval",
                "60",
                "--port",
                "8080",
                "--range",
                "500",
            ])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval,
                listen,
                port,
                range,
                tls,
            } => {
                assert_eq!(dsn.driver, "postgres");
                assert_eq!(interval, 60);
                assert_eq!(listen, None);
                assert_eq!(port, 8080);
                assert_eq!(range, 500);
                assert_eq!(tls.mode, TlsMode::Disable);
            }
        }
    }

    #[test]
    fn test_dispatch_custom_values() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://user:pass@localhost/db",
                "--interval",
                "45",
                "--port",
                "9999",
                "--range",
                "2000",
            ])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval,
                listen,
                port,
                range,
                tls,
            } => {
                assert_eq!(dsn.driver, "mysql");
                assert_eq!(interval, 45);
                assert_eq!(listen, None);
                assert_eq!(port, 9999);
                assert_eq!(range, 2000);
                assert_eq!(tls.mode, TlsMode::Disable);
            }
        }
    }

    #[test]
    fn test_dispatch_with_listen() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "postgres://user:pass@localhost/db",
                "--listen",
                "127.0.0.1",
                "--port",
                "9300",
            ])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval,
                listen,
                port,
                range,
                tls,
            } => {
                assert_eq!(dsn.driver, "postgres");
                assert_eq!(interval, 30);
                assert_eq!(listen, Some("127.0.0.1".parse().unwrap()));
                assert_eq!(port, 9300);
                assert_eq!(range, 100);
                assert_eq!(tls.mode, TlsMode::Disable);
            }
        }
    }

    #[test]
    fn test_dispatch_with_ipv6_listen() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://user:pass@localhost/db",
                "--listen",
                "::",
            ])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval,
                listen,
                port,
                range,
                tls,
            } => {
                assert_eq!(dsn.driver, "mysql");
                assert_eq!(interval, 30);
                assert_eq!(listen, Some("::".parse().unwrap()));
                assert_eq!(port, 9300);
                assert_eq!(range, 100);
                assert_eq!(tls.mode, TlsMode::Disable);
            }
        }
    }

    #[test]
    fn test_dispatch_with_tls() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "postgres://user:pass@tcp(localhost:5432)/db?sslmode=require",
            ])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval: _,
                listen: _,
                port: _,
                range: _,
                tls,
            } => {
                assert_eq!(dsn.driver, "postgres");
                assert_eq!(tls.mode, TlsMode::Require);
                assert!(tls.mode.is_enabled());
            }
        }
    }

    #[test]
    fn test_dispatch_with_tls_full_config() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "postgres://user:pass@tcp(localhost:5432)/db?sslmode=verify-full&sslrootcert=/path/to/ca.crt&sslcert=/path/to/client.crt&sslkey=/path/to/client.key",
            ])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval: _,
                listen: _,
                port: _,
                range: _,
                tls,
            } => {
                assert_eq!(dsn.driver, "postgres");
                assert_eq!(tls.mode, TlsMode::VerifyFull);
                assert_eq!(tls.ca, Some(PathBuf::from("/path/to/ca.crt")));
                assert_eq!(tls.cert, Some(PathBuf::from("/path/to/client.crt")));
                assert_eq!(tls.key, Some(PathBuf::from("/path/to/client.key")));
            }
        }
    }

    #[test]
    fn test_dispatch_with_mysql_ssl_mode() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://root:secret@tcp(localhost:3306)/db?ssl-mode=verify-ca&ssl-ca=/etc/ssl/ca.crt",
            ])
            .unwrap();

        let action = dispatch(&matches).unwrap();
        match action {
            Action::Monitor {
                dsn,
                interval: _,
                listen: _,
                port: _,
                range: _,
                tls,
            } => {
                assert_eq!(dsn.driver, "mysql");
                assert_eq!(tls.mode, TlsMode::VerifyCA);
                assert_eq!(tls.ca, Some(PathBuf::from("/etc/ssl/ca.crt")));
            }
        }
    }

    #[test]
    fn test_dispatch_invalid_listen() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://user:pass@localhost/db",
                "--listen",
                "not-an-ip",
            ])
            .unwrap();

        let result = dispatch(&matches);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid IP address")
        );
    }

    /// Build a DSN without ever writing a literal `scheme://user:pass@host`.
    ///
    /// The `tcp(host:port)` form is required: the `dsn` crate only parses the
    /// database and query parameters when the address is wrapped in a
    /// protocol, and every documented example uses it.
    fn dsn_with(driver: &str, query: &str) -> String {
        format!("{driver}://u:p@tcp(localhost:5432)/db?{query}")
    }

    fn tls_for(driver: &str, query: &str) -> Result<TlsConfig> {
        let matches = commands::new()
            .try_get_matches_from(vec!["dbpulse", "--dsn", &dsn_with(driver, query)])
            .unwrap();

        match dispatch(&matches)? {
            Action::Monitor { tls, .. } => Ok(tls),
        }
    }

    /// Regression: a misspelled `sslmode` used to be swallowed by
    /// `.ok().unwrap_or_default()`, and `TlsMode`'s default is `Disable`.
    ///
    /// The operator asks for a verified TLS connection, gets plaintext, and is
    /// told nothing -- the worst possible outcome for a flag whose only job is
    /// to decide whether credentials cross the network in the clear. Startup
    /// must abort instead.
    #[test]
    fn unknown_ssl_mode_aborts_instead_of_silently_using_plaintext() {
        for query in [
            "sslmode=verify-al",
            "sslmode=on",
            "ssl-mode=VERIFY_FULLY",
            "sslmode=prefer",
        ] {
            let err = tls_for("postgres", query).expect_err(
                "an unparseable TLS mode must abort startup, not fall back to plaintext",
            );

            let rendered = format!("{err:#}");
            assert!(
                rendered.contains("TLS mode"),
                "error should name the offending setting, got: {rendered}"
            );
        }
    }

    /// The DSN documentation advertises `ssl-mode` as a MySQL-style alias, and
    /// dbpulse's own TLS workflow passes `ssl-mode=REQUIRED`. Those values only
    /// parsed as "invalid" before, so the alias silently meant "no TLS".
    #[test]
    fn mysql_ssl_mode_spellings_enable_tls() {
        assert_eq!(
            tls_for("mysql", "ssl-mode=REQUIRED").unwrap().mode,
            TlsMode::Require
        );
        assert_eq!(
            tls_for("mysql", "ssl-mode=VERIFY_CA").unwrap().mode,
            TlsMode::VerifyCA
        );
        assert_eq!(
            tls_for("mysql", "ssl-mode=VERIFY_IDENTITY").unwrap().mode,
            TlsMode::VerifyFull
        );
        assert_eq!(
            tls_for("mysql", "ssl-mode=DISABLED").unwrap().mode,
            TlsMode::Disable
        );
    }

    /// A DSN that says nothing about TLS keeps the historical default.
    #[test]
    fn absent_ssl_mode_still_defaults_to_disable() {
        assert_eq!(
            tls_for("postgres", "connect_timeout=5").unwrap().mode,
            TlsMode::Disable
        );
    }

    /// Regression: the `dsn` crate stores parameter keys verbatim, so
    /// `?SSLMODE=require` missed the case-sensitive `sslmode` lookup and
    /// silently fell back to plaintext -- the fail-open this module must
    /// never allow. Key case must not decide security posture.
    #[test]
    fn ssl_parameter_keys_are_case_insensitive() {
        assert_eq!(
            tls_for("postgres", "SSLMODE=REQUIRED").unwrap().mode,
            TlsMode::Require
        );
        assert_eq!(
            tls_for("mysql", "Ssl-Mode=Verify_Ca").unwrap().mode,
            TlsMode::VerifyCA
        );
        assert_eq!(
            tls_for("mysql", "SSLMODE=VERIFY_IDENTITY").unwrap().mode,
            TlsMode::VerifyFull
        );
        assert_eq!(
            tls_for("postgres", "SSLROOTCERT=/ca.crt").unwrap().ca,
            Some(PathBuf::from("/ca.crt"))
        );
    }

    /// A bad value under a differently-cased key must still fail closed.
    #[test]
    fn uppercase_sslmode_with_a_bad_value_aborts_startup() {
        let err = tls_for("postgres", "SSLMODE=definitely-not-a-mode")
            .expect_err("an invalid mode must fail closed regardless of key case");
        assert!(
            format!("{err:#}").contains("TLS mode"),
            "error should name the offending setting, got: {err:#}"
        );
    }
}
