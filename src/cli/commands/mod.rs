use clap::{
    Arg, ColorChoice, Command,
    builder::styling::{AnsiColor, Effects, Styles},
};

fn dsn_arg() -> Arg {
    Arg::new("dsn")
        .env("DBPULSE_DSN")
        .help("<mysql|postgres>://<username>:<password>@tcp(<host>:<port>)/<database>?sslmode=<mode>")
        .long("dsn")
        .long_help(
            "Database connection string with optional TLS parameters:\n\n\
            Format: <driver>://<user>:<pass>@tcp(<host>:<port>)/<db>?param1=value1&param2=value2\n\n\
            TLS Parameters (query string):\n\
            - sslmode: disable|require|verify-ca|verify-full (default: disable)\n\
            - sslrootcert or sslca: Path to CA certificate file\n\
            - sslcert: Path to client certificate file\n\
            - sslkey: Path to client private key file\n\n\
            Examples:\n\
            postgres://user:pass@tcp(localhost:5432)/db?sslmode=require\n\
            mysql://root:secret@tcp(db.example.com:3306)/prod?sslmode=verify-full&sslca=/etc/ssl/ca.crt"
        )
        .short('d')
        .required(true)
}

fn interval_arg() -> Arg {
    Arg::new("interval")
        .default_value("30")
        .env("DBPULSE_INTERVAL")
        .help("number of seconds between checks")
        .long("interval")
        .short('i')
        // 0 would leave no pause between checks at all, turning a pulse check
        // into a load generator.
        .value_parser(clap::value_parser!(u16).range(1..))
}

fn listen_arg() -> Arg {
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
        .value_name("IP")
}

fn port_arg() -> Arg {
    Arg::new("port")
        .default_value("9300")
        .env("DBPULSE_PORT")
        .help("listening port for /metrics")
        .long("port")
        .short('p')
        .value_parser(clap::value_parser!(u16))
}

fn range_arg() -> Arg {
    Arg::new("range")
        .default_value("100")
        .env("DBPULSE_RANGE")
        .help("The upper limit of the ID range")
        .long("range")
        .short('r')
        // 0 makes the random id sampler panic on every single iteration, and
        // the id column is a signed INT in both schemas, so larger ranges
        // produce ids that cannot be written at all.
        .value_parser(clap::value_parser!(u32).range(1..=2_147_483_647))
}

/// Pure clap command definitions with zero business logic
#[must_use]
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
        .arg(dsn_arg())
        .arg(interval_arg())
        .arg(listen_arg())
        .arg(port_arg())
        .arg(range_arg())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

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
    fn test_interval_zero_is_rejected() {
        // Regression: `--interval 0` left no pause between checks at all,
        // driving ~25 iterations/second at the database.
        let err = new()
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://u:p@localhost/d",
                "-i",
                "0",
            ])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_range_zero_is_rejected() {
        // Regression: `--range 0` made the random id sampler panic on every
        // iteration ("cannot sample empty range"), and the panic handler simply
        // re-entered the loop forever without ever making progress.
        let err = new()
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://u:p@localhost/d",
                "-r",
                "0",
            ])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_minimum_interval_and_range_are_accepted() {
        let m = new()
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://u:p@localhost/d",
                "-i",
                "1",
                "-r",
                "1",
            ])
            .unwrap();
        assert_eq!(m.get_one::<u16>("interval").copied(), Some(1));
        assert_eq!(m.get_one::<u32>("range").copied(), Some(1));
    }

    #[test]
    fn test_range_above_int_max_is_rejected() {
        // The id column is a signed INT in both schemas; ids beyond
        // i32::MAX cannot be written, so roughly every other check failed
        // (a graceful error on PostgreSQL, server error 1264 on MySQL).
        let err = new()
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://u:p@localhost/d",
                "-r",
                "2147483648",
            ])
            .unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn test_range_at_int_max_is_accepted() {
        let m = new()
            .try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://u:p@localhost/d",
                "-r",
                "2147483647",
            ])
            .unwrap();
        assert_eq!(m.get_one::<u32>("range").copied(), Some(2_147_483_647));
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
