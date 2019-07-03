use rand::Rng;
use std::{error, fmt};
use uuid::Uuid;

#[derive(Debug)]
pub enum Error {
    MySQL(mysql::Error),
    NotMatching(String),
    RowError(mysql::FromRowError),
    RowExpected,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::MySQL(ref err) => err.fmt(f),
            Error::NotMatching(ref err) => err.fmt(f),
            Error::RowError(ref err) => err.fmt(f),
            Error::RowExpected => write!(f, "row expected"),
        }
    }
}

impl error::Error for Error {}

impl From<mysql::Error> for Error {
    fn from(err: mysql::Error) -> Self {
        Error::MySQL(err)
    }
}

impl From<mysql::FromRowError> for Error {
    fn from(err: mysql::FromRowError) -> Self {
        Error::RowError(err)
    }
}

pub struct Queries {
    pool: mysql::Pool,
}

pub fn new(pool: mysql::Pool) -> Queries {
    return Queries { pool: pool };
}

impl Queries {
    pub fn test_rw(&self, now: u64) -> Result<isize, Error> {
        let pool = &self.pool.clone();

        // create table
        pool.prep_exec(
            r#"CREATE TABLE IF NOT EXISTS dbpulse_rw (
        id INT NOT NULL,
        t1 INT(11) NOT NULL ,
        t2 timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
        uuid CHAR(36) CHARACTER SET ascii,
        UNIQUE KEY(uuid),
        PRIMARY KEY(id)) ENGINE=InnoDB"#,
            (),
        )?;

        // write into table
        let num = rand::thread_rng().gen_range(0, 100);
        let uuid = Uuid::new_v4();
        pool.prepare("INSERT INTO dbpulse_rw (id, t1, uuid) VALUES (?, ?, ?) ON DUPLICATE KEY UPDATE t1=?, uuid=?")?
            .execute((num, now, uuid.to_string(), now, uuid.to_string()))?;

        // check if stored record matches
        let result = pool
            .prepare("SELECT t1, uuid FROM dbpulse_rw Where id=?")?
            .execute((num,))?
            .last()
            .ok_or(Error::RowExpected)??;
        let (t1, v4) = mysql::from_row_opt::<(u64, String)>(result)?;
        if now != t1 || uuid.to_string() != v4 {
            return Err(Error::NotMatching(format!(
                "({}, {}) != ({},{})",
                now, uuid, t1, v4
            )));
        }

        // check transaction setting all records to 0
        let mut tr = pool.start_transaction(false, None, None)?;
        tr.prep_exec("UPDATE dbpulse_rw SET t1=?", (0,))?;
        let rows = tr.prep_exec("SELECT t1 FROM dbpulse_rw", ())?;
        for row in rows {
            let row = row.map_err(Error::MySQL)?;
            let row = mysql::from_row_opt::<u64>(row)?;
            if row != 0 {
                return Err(Error::NotMatching(format!("{} != {}", row, 0)));
            }
        }
        tr.rollback()?;

        // update record 1 with now
        pool.prepare(
            "INSERT INTO dbpulse_rw (id, t1, uuid) VALUES (0, ?, UUID()) ON DUPLICATE KEY UPDATE t1=?",
        )?
        .execute((now, now))?;

        // get elapsed time
        let row = pool
            .prep_exec(
                "SELECT TIMESTAMPDIFF(SECOND, FROM_UNIXTIME(t1), t2) from dbpulse_rw where id=0",
                (),
            )?
            .last()
            .ok_or(Error::RowExpected)??;
        Ok(mysql::from_row_opt::<isize>(row)?)
    }

    pub fn drop_table(&self) -> Result<(), Error> {
        let pool = &self.pool.clone();
        pool.prep_exec("DROP TABLE dbpulse_rw", ())?;
        Ok(())
    }

    pub fn get_user_time_state_info(&self) -> Result<(String, i64, String, String, i64), Error> {
        // to lock for writes
        // FLUSH TABLES WITH READ LOCK;
        let pool = &self.pool.clone();
        let row= pool.prepare("SELECT user, time, db, state, memory_used FROM information_schema.processlist WHERE command != 'Sleep' AND info LIKE 'alter%' AND time >= ? ORDER BY time DESC, id LIMIT 1")?
        .execute((5,))?
        .last()
        .ok_or(Error::RowExpected)??;
        Ok(mysql::from_row_opt::<(String, i64, String, String, i64)>(
            row,
        )?)
    }
}
