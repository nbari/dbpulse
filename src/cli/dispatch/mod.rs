use crate::cli::actions::Action;
use crate::tls::{TlsConfig, TlsMode};
use anyhow::{Context, Result};
use clap::ArgMatches;
use std::net::IpAddr;
use std::path::PathBuf;

/// Convert `ArgMatches` into typed Action enum with validation
///
/// # Errors
///
/// Returns an error if the DSN is invalid or required parameters are missing
#[allow(clippy::needless_pass_by_value)]
pub fn dispatch(matches: ArgMatches) -> Result<Action> {
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

    // Extract TLS configuration
    let tls_mode = matches
        .get_one::<String>("tls-mode")
        .and_then(|m| m.parse::<TlsMode>().ok())
        .unwrap_or_default();

    let tls = TlsConfig {
        mode: tls_mode,
        ca: matches.get_one::<String>("tls-ca").map(PathBuf::from),
        cert: matches.get_one::<String>("tls-cert").map(PathBuf::from),
        key: matches.get_one::<String>("tls-key").map(PathBuf::from),
    };

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
    use super::*;
    use crate::cli::commands;

    #[test]
    fn test_dispatch_valid_mysql() {
        let cmd = commands::new();
        let matches = cmd
            .try_get_matches_from(vec!["dbpulse", "--dsn", "mysql://user:pass@localhost/db"])
            .unwrap();

        let action = dispatch(matches).unwrap();
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

        let action = dispatch(matches).unwrap();
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

        let action = dispatch(matches).unwrap();
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

        let action = dispatch(matches).unwrap();
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

        let action = dispatch(matches).unwrap();
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
                "postgres://user:pass@localhost/db",
                "--tls-mode",
                "require",
            ])
            .unwrap();

        let action = dispatch(matches).unwrap();
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

        let result = dispatch(matches);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid IP address")
        );
    }
}
