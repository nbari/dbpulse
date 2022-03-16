# dbpulse

command line tool to monitor database performance

## grants

Set this grants to the MySQL user:

    GRANT SELECT, SHOW VIEW, PROCESS ON *.* TO 'dbpulse'@'%';

Environment variables required:

    DSN

Optional:

    INTERVAL=30
    TIMEOUT=3
    PORT=9300
