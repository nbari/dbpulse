use clap::{
    builder::styling::{AnsiColor, Effects, Styles},
    Arg, ColorChoice, Command,
};

#[must_use]
// returns (v46, port, interval, opts)
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
                .default_value("9300")
                .env("DBPULSE_RANGE")
                .help("The upper limit of the ID range")
                .long("range")
                .short('r')
                .default_value("100")
                .value_parser(clap::value_parser!(u32)),
        )
}

#[cfg(test)]
mod tests {
    mod new {
        use super::super::new;

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
            let cmd = new();
            let matches = cmd.try_get_matches_from(vec!["dbpulse"]);
            assert!(matches.is_err());
        }

        #[test]
        fn test_new_args_mysql() {
            let cmd = new();
            let matches = cmd.try_get_matches_from(vec![
                "dbpulse",
                "--dsn",
                "mysql://user:pass@localhost/db",
            ]);
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
        fn test_new_arga_range() {
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
            assert_eq!(m.get_one::<i32>("range").copied(), Some(1000));
        }
    }
}
