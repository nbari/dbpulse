# dbpulse

command line tool to monitor database performance

## grants

Set this grants to the mysql user:

    GRANT SELECT, SHOW VIEW, PROCESS ON *.* TO 'dbpulse'@'%';
