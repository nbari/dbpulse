[![build](https://github.com/nbari/dbpulse/actions/workflows/build.yml/badge.svg)](https://github.com/nbari/dbpulse/actions/workflows/build.yml)
[![crates.io](https://img.shields.io/crates/v/dbpulse.svg)](https://crates.io/crates/dbpulse)

# dbpulse

`dbpulse` will run a set of queries in a defined interval, in order to
dynamically test if the database is available mainly for writes, it exposes a
`/metrics` endpoint the one can be used together with `Prometheus` and create
alerts when the database is not available, this is to cover HALT/LOCK cases in
Galera clusters in where a `DDL` could stale the whole cluster or flow-control
kicks in and the database could not be receiving `COMMITS/WRITE`.


## How to use it

Run it as a client, probably hitting your load balancer so that you can test
like if you where a client, you need to pass the `DSN` or see it up as an
environment var.

## /metrics

The `dbpulse_pulse` is a gauge will return 1 when DB is healthy (read/write) OK,

The calculate the runtime:

    sum(rate(dbpulse_runtime_sum[5m])) / sum(rate(dbpulse_runtime_count[5m]))


Current options:

```
command line tool to monitor that database is available for read & write

Usage: dbpulse [OPTIONS] --dsn <dsn>

Options:
  -d, --dsn <dsn>            <mysql|postgres>://<username>:<password>@tcp(<host>:<port>)/<database> [env: DBPULSE_DSN=postgres://postgres:secret@tcp(localhost)/dbpulse]
  -i, --interval <interval>  number of seconds between checks [env: DBPULSE_INTERVAL=] [default: 30]
  -p, --port <port>          listening port for /metrics [env: DBPULSE_PORT=] [default: 9300]
  -r, --range <range>        The upper limit of the ID range [env: DBPULSE_RANGE=] [default: 100]
  -h, --help                 Print help
  -V, --version              Print version

```

Example:

```sh
dbpulse --dsn "postgres://postgres:secret@tcp(10.10.0.10)/dbpulse" -r 2880
```

> the app tries to create the database if it does not exist (depends on the user permissions)

# rpm

To create an RPM package:

```sh
just rpm
```

Then you need to copy the `dbpulse*.x86_64.rpm`:

```sh
cp target/generate-rpm/dbpulse-*-x86_64.rpm /host
```

```sh
