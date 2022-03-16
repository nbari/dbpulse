# dbpulse

`dbpulse` will run a set of queries in a defined interval, in order to
dynamically test if the database is available mainly for writes, it exposes a
`/metrics` endpoint the one can be used together with `Prometheus` and create
alerts when the database is not available, this is to cover HALT/LOCK cases in
Galera cluster in where a DDL could stale the whole cluster, flow-control kicks
in and the database could not be receiving temporally `COMMITS`


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
USAGE:
    dbpulse [OPTIONS] --dsn <dsn>

OPTIONS:
        --46                     listen in both IPv4 and IPv6
        --dsn <dsn>              mysql://<username>:<password>@tcp(<host>:<port>)/<database> [env: DSN=]
    -h, --help                   Print help information
    -i, --interval <interval>    number of seconds between checks [env: INTERVAL=] [default: 30]
    -p, --port <port>            listening port for /metrics [env: PORT=] [default: 9300]
    -V, --version                Print version information
```
