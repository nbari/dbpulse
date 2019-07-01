use rand::Rng;
use std::{error, fmt};

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
    pub fn test_rw(&self, now: u64) -> Result<usize, Error> {
        let pool = &self.pool.clone();

        // create table
        pool.prep_exec(
            r#"CREATE TABLE IF NOT EXISTS dbpulse_rw (
        id INT NOT NULL,
        t1 INT(11) NOT NULL,
        t2 timestamp NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
        PRIMARY KEY(id)) ENGINE=InnoDB"#,
            (),
        )?;

        // write into table
        let mut stmt = pool.prepare(
            "INSERT INTO dbpulse_rw (id, t1) VALUES (?, ?) ON DUPLICATE KEY UPDATE t1=?",
        )?;

        let num = rand::thread_rng().gen_range(0, 100);
        stmt.execute((num, now, now))?;

        // check if stored record matches
        let mut stmt = pool.prepare("SELECT t1 FROM dbpulse_rw Where id=?")?;
        let result = stmt.execute((num,))?.last().ok_or(Error::RowExpected)??;
        let row = mysql::from_row_opt::<u64>(result)?;
        if now != row {
            return Err(Error::NotMatching(format!("{} != {}", now, row)));
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
        pool.prepare("INSERT INTO dbpulse_rw (id, t1) VALUES (0, ?) ON DUPLICATE KEY UPDATE t1=?")?
            .execute((now, now))?;

        // get elapsed time
        let row = pool
            .prep_exec(
                "SELECT TIMESTAMPDIFF(SECOND, FROM_UNIXTIME(t1), t2) from dbpulse_rw where id=0",
                (),
            )?
            .last()
            .ok_or(Error::RowExpected)??;
        Ok(mysql::from_row_opt::<usize>(row)?)
    }
}
