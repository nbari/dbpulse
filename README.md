# dbpulse

command line tool to monitor database performance

## grants

Set this grants to the MySQL user:

    GRANT SELECT, SHOW VIEW, PROCESS ON *.* TO 'dbpulse'@'%';

Environment variables required:

    DSN""
    ENVIRONMENT=""
    EVERY=30
    RW_TIMEOUT=3
    SLACK_WEBHOOK_URL=""
    THRESHOLD_HEALTHY=2
    THRESHOLD_UNHEALTHY=3
