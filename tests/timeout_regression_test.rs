//! End-to-end regression tests for the monitoring loop's failure handling.
//!
//! These drive the real binary, because both bugs they cover were invisible at
//! the unit level: they were about what a Prometheus scrape sees while the
//! database is unreachable.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// A TCP server that completes the accept and then never sends a byte.
///
/// This is what a wedged database looks like from the client side: a hung host,
/// a stuck proxy, or a VIP mid-failover. It is *not* a refused connection --
/// `connect()` succeeds, so anything without a deadline waits forever.
struct BlackHoleServer {
    port: u16,
}

impl BlackHoleServer {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();

        thread::spawn(move || {
            // Holding the streams is the entire point: dropping one would close
            // the connection and let the client fail fast instead of hanging.
            #[allow(clippy::collection_is_never_read)]
            let mut held = Vec::new();
            for stream in listener.incoming() {
                match stream {
                    // Hold the connection open and stay silent.
                    Ok(stream) => held.push(stream),
                    Err(_) => break,
                }
            }
        });

        Self { port }
    }
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Scrape /metrics over a raw socket, to avoid pulling in an HTTP client.
fn scrape(port: u16) -> Option<String> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).ok()?;
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"GET /metrics HTTP/1.0\r\nHost: localhost\r\n\r\n")
        .ok()?;
    let mut body = String::new();
    stream.read_to_string(&mut body).ok()?;
    Some(body)
}

fn wait_for_metrics(port: u16, deadline: Duration) -> String {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if let Some(body) = scrape(port)
            && body.contains("dbpulse_")
        {
            return body;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("dbpulse did not serve metrics within {deadline:?}");
}

struct Dbpulse(Child);

impl Drop for Dbpulse {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn spawn_dbpulse(db_port: u16, metrics_port: u16, interval: &str) -> Dbpulse {
    let child = Command::new(env!("CARGO_BIN_EXE_dbpulse"))
        .args([
            "--dsn",
            &format!("mysql://user:pass@tcp(127.0.0.1:{db_port})/testdb"),
            "--interval",
            interval,
            "--port",
            &metrics_port.to_string(),
            "--listen",
            "127.0.0.1",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to spawn dbpulse");
    Dbpulse(child)
}

/// Every metric must exist from the first scrape, before any check completes.
///
/// Regression: metrics are `LazyLock` and used to register only on first use,
/// so until an iteration finished `dbpulse_pulse` was *absent* rather than 0.
/// An alert written as `dbpulse_pulse == 0` silently never fires on an absent
/// series, which meant a database that was down from the start produced no
/// alert at all.
#[test]
fn metrics_are_registered_before_the_first_check_completes() {
    let db = BlackHoleServer::start();
    let metrics_port = free_port();
    let _dbpulse = spawn_dbpulse(db.port, metrics_port, "30");

    // The check against a black hole cannot complete, so anything visible here
    // is there because startup registered it.
    let body = wait_for_metrics(metrics_port, Duration::from_secs(20));

    // Whole sample lines, not bare metric names: a `# HELP dbpulse_errors_total`
    // header is emitted as soon as the collector is registered, so
    // `body.contains("dbpulse_errors_total")` passes even with no label child
    // at all. Alerts match on series, so the series is what must be pinned.
    for sample in [
        "dbpulse_pulse 0",
        "dbpulse_iterations_total{database=\"mysql\",status=\"success\"} 0",
        "dbpulse_iterations_total{database=\"mysql\",status=\"error\"} 0",
        "dbpulse_errors_total{database=\"mysql\",error_type=\"authentication\"} 0",
        "dbpulse_errors_total{database=\"mysql\",error_type=\"timeout\"} 0",
        "dbpulse_errors_total{database=\"mysql\",error_type=\"connection\"} 0",
        "dbpulse_errors_total{database=\"mysql\",error_type=\"transaction\"} 0",
        "dbpulse_errors_total{database=\"mysql\",error_type=\"query\"} 0",
        "dbpulse_last_success_timestamp_seconds{database=\"mysql\"} 0",
        "dbpulse_database_readonly{database=\"mysql\"} 0",
        "dbpulse_runtime_last_milliseconds{database=\"mysql\"} 0",
        "dbpulse_table_recreated_total{database=\"mysql\"} 0",
        "dbpulse_panics_recovered_total 0",
    ] {
        assert!(
            body.contains(sample),
            "`{sample}` missing from /metrics before the first check completed, got:\n{body}"
        );
    }
}

/// A wedged database must produce an error, not an indefinite stall.
///
/// Regression: nothing bounded the health check. The `SET SESSION` statement
/// and lock timeouts only apply once a connection exists, so a server that
/// accepts TCP and then goes silent stalled the loop forever -- no iteration,
/// no error metric, no output, indefinitely.
#[test]
fn wedged_database_times_out_and_is_recorded_as_an_error() {
    let db = BlackHoleServer::start();
    let metrics_port = free_port();
    // interval 1 => the check deadline floors at 5s (never below the server-side
    // statement timeout), so one full iteration must land well inside 30s.
    let _dbpulse = spawn_dbpulse(db.port, metrics_port, "1");

    wait_for_metrics(metrics_port, Duration::from_secs(20));

    let start = Instant::now();
    let deadline = Duration::from_secs(30);
    let mut last = String::new();
    while start.elapsed() < deadline {
        last = scrape(metrics_port).unwrap_or_default();
        if last.contains(r#"dbpulse_iterations_total{database="mysql",status="error"} 1"#) {
            let elapsed = start.elapsed();
            assert!(
                last.contains(r#"dbpulse_errors_total{database="mysql",error_type="timeout"}"#),
                "the failure should be classified as a timeout, got:\n{last}"
            );
            assert!(
                last.contains("dbpulse_pulse 0"),
                "pulse should be 0 after a failed check, got:\n{last}"
            );
            assert!(
                elapsed < deadline,
                "iteration took {elapsed:?}, expected it to be bounded"
            );
            return;
        }
        thread::sleep(Duration::from_millis(200));
    }

    panic!("no failed iteration was recorded within {deadline:?}; last scrape:\n{last}");
}
