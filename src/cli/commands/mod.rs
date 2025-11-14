use clap::{
    Arg, ColorChoice, Command,
    builder::styling::{AnsiColor, Effects, Styles},
};

/// Pure clap command definitions with zero business logic
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn new() -> Command {
    let styles = Styles::styled()
        .header(AnsiColor::Yellow.on_default() | Effects::BOLD)
        .usage(AnsiColor::Green.on_default() | Effects::BOLD)
        .literal(AnsiColor::Blue.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Green.on_default());

    Command::new(env!("CARGO_PKG_NAME"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .version(env!("CARGO_PKG_VERSION"))
        .color(ColorChoice::Auto)
        .styles(styles)
        .arg(
            Arg::new("dsn")
                .env("DBPULSE_DSN")
                .help("<mysql|postgres>://<username>:<password>@tcp(<host>:<port>)/<database>")
                .long("dsn")
                .short('d')
                .required(true),
        )
        .arg(
            Arg::new("interval")
                .default_value("30")
                .env("DBPULSE_INTERVAL")
                .help("number of seconds between checks")
                .long("interval")
                .short('i')
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            Arg::new("listen")
                .env("DBPULSE_LISTEN")
                .help("IP address to bind to (default: [::]:port, accepts both IPv6 and IPv4)")
                .long("listen")
                .long_help(
                    "IP address to bind to:\n\
                    Not specified (default) binds to [::]:port which accepts both IPv6 and IPv4 connections.\n\
                    Falls back to 0.0.0.0:port if IPv6 is unavailable.\n\n\
                    Specific IPv4 examples: '0.0.0.0', '127.0.0.1'\n\
                    Specific IPv6: '::', '::1'\n\n\
                    Usage examples:\n\
                    - `--listen 0.0.0.0` binds IPv4 only\n\
                    - `--listen ::` binds IPv6 (typically accepts IPv4 too)\n\n\
                    Note: binding to [::] usually accepts both IPv6 and IPv4 through \
                    IPv4-mapped addresses on dual-stack systems."
                )
                .short('l')
                .value_name("IP"),
        )
        .arg(
            Arg::new("port")
                .default_value("9300")
                .env("DBPULSE_PORT")
                .help("listening port for /metrics")
                .long("port")
                .short('p')
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            Arg::new("range")
                .default_value("100")
                .env("DBPULSE_RANGE")
                .help("The upper limit of the ID range")
                .long("range")
                .short('r')
                .value_parser(clap::value_parser!(u32)),
        )
        .arg(
            Arg::new("tls-mode")
                .env("DBPULSE_TLS_MODE")
                .help("TLS/SSL mode: disable, require, verify-ca, verify-full")
                .long("tls-mode")
                .long_help(
                    "TLS/SSL connection mode:\n\n\
                    - disable: No TLS (default)\n\
                    - require: TLS required, no certificate verification\n\
                    - verify-ca: Verify server certificate against CA\n\
                    - verify-full: Verify certificate and hostname\n\n\
                    MySQL/MariaDB: Maps to ssl-mode (DISABLED, REQUIRED, VERIFY_CA, VERIFY_IDENTITY)\n\
                    PostgreSQL: Maps to sslmode (disable, require, verify-ca, verify-full)\n\n\
                    Note: TLS monitoring provides detailed metrics including:\n\
                    - Handshake duration and success/failure rates\n\
                    - TLS version negotiated (TLSv1.2, TLSv1.3)\n\
                    - Cipher suite in use\n\
                    - Certificate expiration monitoring"
                )
                .value_name("MODE")
                .value_parser(["disable", "require", "verify-ca", "verify-full"]),
        )
        .arg(
            Arg::new("tls-ca")
                .env("DBPULSE_TLS_CA")
                .help("Path to CA certificate file for TLS verification")
                .long("tls-ca")
                .long_help(
                    "Path to Certificate Authority (CA) certificate file.\n\
                    Required for verify-ca and verify-full modes.\n\n\
                    Example: /etc/ssl/certs/ca-certificates.crt"
                )
                .value_name("PATH")
                .requires("tls-mode"),
        )
        .arg(
            Arg::new("tls-cert")
                .env("DBPULSE_TLS_CERT")
                .help("Path to client certificate file for TLS client authentication")
                .long("tls-cert")
                .long_help(
                    "Path to client certificate file for mutual TLS authentication.\n\
                    Must be used together with --tls-key.\n\n\
                    Example: /etc/dbpulse/client-cert.pem"
                )
                .value_name("PATH")
                .requires("tls-key"),
        )
        .arg(
            Arg::new("tls-key")
                .env("DBPULSE_TLS_KEY")
                .help("Path to client private key file for TLS client authentication")
                .long("tls-key")
                .long_help(
                    "Path to client private key file for mutual TLS authentication.\n\
                    Must be used together with --tls-cert.\n\n\
                    Example: /etc/dbpulse/client-key.pem"
                )
                .value_name("PATH")
                .requires("tls-cert"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let cmd = new();
        assert_eq!(cmd.get_name(), "dbpulse");
        assert_eq!(
            cmd.get_about().unwrap().to_string(),
            env!("CARGO_PKG_DESCRIPTION")
        );
        assert_eq!(
            cmd.get_version().unwrap().to_string(),
            env!("CARGO_PKG_VERSION")
        );
    }

    #[test]
    fn test_new_no_args() {
        // Temporarily remove environment variable to test required DSN
        let original_dsn = std::env::var("DBPULSE_DSN").ok();
        // SAFETY: This test runs in isolation and we restore the variable afterward
        unsafe {
            std::env::remove_var("DBPULSE_DSN");
        }

        let cmd = new();
        let matches = cmd.try_get_matches_from(vec!["dbpulse"]);
        assert!(matches.is_err());

        // Restore original environment variable if it existed
        if let Some(dsn) = original_dsn {
            // SAFETY: Restoring the original state
            unsafe {
                std::env::set_var("DBPULSE_DSN", dsn);
            }
        }
    }

    #[test]
    fn test_new_args_mysql() {
        let cmd = new();
        let matches =
            cmd.try_get_matches_from(vec!["dbpulse", "--dsn", "mysql://user:pass@localhost/db"]);
        assert!(matches.is_ok());

        let m = matches.unwrap();
        assert_eq!(
            m.get_one("dsn"),
            Some(&String::from("mysql://user:pass@localhost/db"))
        );
        assert_eq!(m.get_one::<u16>("interval").copied(), Some(30));
        assert_eq!(m.get_one::<u16>("port").copied(), Some(9300));
    }

    #[test]
    fn test_new_args_postgres() {
        let cmd = new();
        let matches = cmd.try_get_matches_from(vec![
            "dbpulse",
            "--dsn",
            "postgres://user:pass@localhost/db",
        ]);
        assert!(matches.is_ok());

        let m = matches.unwrap();
        assert_eq!(
            m.get_one("dsn"),
            Some(&String::from("postgres://user:pass@localhost/db"))
        );
        assert_eq!(m.get_one::<u16>("interval").copied(), Some(30));
        assert_eq!(m.get_one::<u16>("port").copied(), Some(9300));
    }

    #[test]
    fn test_new_args_range() {
        let cmd = new();
        let matches = cmd.try_get_matches_from(vec![
            "dbpulse",
            "--dsn",
            "postgres://user:pass@localhost/db",
            "--range",
            "1000",
        ]);
        assert!(matches.is_ok());

        let m = matches.unwrap();
        assert_eq!(m.get_one::<u32>("range").copied(), Some(1000));
    }
}
