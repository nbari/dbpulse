use clap::{Arg, Command};
use std::process;

#[must_use]
// returns (v46, port, interval, opts)
pub fn new() -> (bool, u16, i64, mysql_async::OptsBuilder) {
    let matches = Command::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .arg(
            Arg::new("dsn")
                .env("DSN")
                .help("mysql://<username>:<password>@tcp(<host>:<port>)/<database>")
                .long("dsn")
                .required(true),
        )
        .arg(
            Arg::new("interval")
                .default_value("30")
                .env("INTERVAL")
                .help("number of seconds between checks")
                .long("interval")
                .short('i')
                .value_parser(clap::value_parser!(i64)),
        )
        .arg(
            Arg::new("port")
                .default_value("9300")
                .env("PORT")
                .help("listening port for /metrics")
                .long("port")
                .short('p')
                .value_parser(clap::value_parser!(u16)),
        )
        .arg(
            Arg::new("v46")
                .help("listen in both IPv4 and IPv6")
                .long("46"),
        )
        .get_matches();

    // prepare DSN for the mysql pool
    let dsn = matches
        .get_one("dsn")
        .map(|s: &String| s)
        .unwrap_or_else(|| {
            eprintln!("DSN is required");
            process::exit(1);
        });
    let dsn = dsn::parse(dsn).unwrap_or_else(|e| {
        eprintln!("{}", e);
        process::exit(1);
    });

    // mysql ssl options
    let mut ssl_opts = None;
    if let Some(tls) = dsn.params.get("tls") {
        if *tls == "skip-verify" {
            ssl_opts = Some(mysql_async::SslOpts::default().with_danger_accept_invalid_certs(true));
        }
    };

    let opts = mysql_async::OptsBuilder::default()
        .user(dsn.username)
        .pass(dsn.password)
        .db_name(dsn.database)
        .ip_or_hostname(dsn.host.unwrap_or_else(|| String::from("127.0.0.1")))
        .tcp_port(dsn.port.unwrap_or(3306))
        .socket(dsn.socket)
        .stmt_cache_size(0)
        .ssl_opts(ssl_opts);

    let port = matches.get_one::<u16>("port").copied().unwrap_or(9200);

    let interval = matches.get_one::<i64>("interval").copied().unwrap_or(30);

    (matches.contains_id("v46"), port, interval, opts)
}
