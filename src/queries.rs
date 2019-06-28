use std::{error, fmt};

#[derive(Debug)]
pub enum Error {
    MySQL(mysql::Error),
    NotMatching(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::MySQL(ref err) => err.fmt(f),
            Error::NotMatching(ref err) => err.fmt(f),
        }
    }
}

impl error::Error for Error {}

impl From<mysql::Error> for Error {
    fn from(err: mysql::Error) -> Self {
        Error::MySQL(err)
    }
}

pub struct Queries {
    pool: mysql::Pool,
}

pub fn new(pool: mysql::Pool) -> Queries {
    return Queries { pool: pool };
}

impl Queries {
    pub fn test_rw(&self, now: u64) -> Result<(), Error> {
        let pool = &self.pool.clone();

        // create table
        pool.prep_exec("CREATE TABLE IF NOT EXISTS dbpulse_rw (id INT NOT NULL, t INT(11) NOT NULL, PRIMARY KEY(id))", ())?;

        // write into table
        let mut stmt = pool
            .prepare("INSERT INTO dbpulse_rw (id, t) VALUES (1, ?) ON DUPLICATE KEY UPDATE t=?")?;
        stmt.execute((now, now))?;

        let rows = pool.prep_exec("SELECT t FROM dbpulse_rw WHERE id=1", ())?;
        for row in rows {
            let row = row.map_err(Error::MySQL)?;
            let row = mysql::from_row_opt::<u64>(row).map_err(|e| Error::MySQL(e.into()))?;
            if now != row {
                return Err(Error::NotMatching("no matching...".into()));
            }
        }
        Ok(())
    }
}
