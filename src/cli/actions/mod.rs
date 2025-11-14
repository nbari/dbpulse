mod run;

use crate::tls::TlsConfig;
use dsn::DSN;
use std::net::IpAddr;

/// Action enum representing each possible command
#[derive(Debug)]
pub enum Action {
    Monitor {
        dsn: DSN,
        interval: u16,
        listen: Option<IpAddr>,
        port: u16,
        range: u32,
        tls: TlsConfig,
    },
}

impl Action {
    /// Execute the action
    ///
    /// # Errors
    ///
    /// Returns an error if the action fails to execute
    pub async fn execute(self) -> anyhow::Result<()> {
        run::execute(self).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tls::TlsMode;

    #[test]
    fn test_action_debug() {
        let dsn = dsn::parse("postgres://localhost/test").unwrap();
        let action = Action::Monitor {
            dsn,
            interval: 30,
            listen: None,
            port: 8080,
            range: 100,
            tls: TlsConfig::default(),
        };

        // Test Debug trait
        let debug_str = format!("{action:?}");
        assert!(debug_str.contains("Monitor"));
    }

    #[test]
    fn test_action_with_ipv4_listen() {
        let dsn = dsn::parse("mysql://localhost/test").unwrap();
        let listen_addr = "127.0.0.1".parse::<IpAddr>().unwrap();
        let action = Action::Monitor {
            dsn,
            interval: 60,
            listen: Some(listen_addr),
            port: 9090,
            range: 1000,
            tls: TlsConfig {
                mode: TlsMode::Disable,
                ca: None,
                cert: None,
                key: None,
            },
        };

        match action {
            Action::Monitor { listen, .. } => {
                assert!(listen.is_some());
                assert_eq!(listen.unwrap().to_string(), "127.0.0.1");
            }
        }
    }

    #[test]
    fn test_action_with_ipv6_listen() {
        let dsn = dsn::parse("postgres://localhost/test").unwrap();
        let listen_addr = "::1".parse::<IpAddr>().unwrap();
        let action = Action::Monitor {
            dsn,
            interval: 15,
            listen: Some(listen_addr),
            port: 3000,
            range: 50,
            tls: TlsConfig::default(),
        };

        match action {
            Action::Monitor { listen, .. } => {
                assert!(listen.is_some());
                assert_eq!(listen.unwrap().to_string(), "::1");
            }
        }
    }

    #[test]
    fn test_action_with_tls_config() {
        let dsn = dsn::parse("postgres://localhost/test").unwrap();
        let tls = TlsConfig {
            mode: TlsMode::VerifyFull,
            ca: Some("/path/to/ca.crt".into()),
            cert: Some("/path/to/client.crt".into()),
            key: Some("/path/to/client.key".into()),
        };
        let action = Action::Monitor {
            dsn,
            interval: 30,
            listen: None,
            port: 8080,
            range: 100,
            tls,
        };

        match action {
            Action::Monitor { tls, .. } => {
                assert_eq!(tls.mode, TlsMode::VerifyFull);
                assert_eq!(tls.ca, Some("/path/to/ca.crt".into()));
                assert_eq!(tls.cert, Some("/path/to/client.crt".into()));
                assert_eq!(tls.key, Some("/path/to/client.key".into()));
            }
        }
    }
}
