use std::{error::Error, fmt};

#[derive(Debug)]
pub enum QueriesError {
    MySQL(mysql::Error),
    NotMatching,
}

impl fmt::Display for QueriesError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            QueriesError::MySQL(ref err) => err.fmt(f),
            QueriesError::NotMatching => write!(f, "Not matching"),
        }
    }
}

impl Error for QueriesError {
    fn description(&self) -> &str {
        match *self {
            QueriesError::MySQL(ref err) => err.description(),
            QueriesError::NotMatching => "Unknown error!",
        }
    }
}

impl From<mysql::Error> for QueriesError {
    fn from(cause: mysql::Error) -> QueriesError {
        QueriesError::MySQL(cause)
    }
}

pub struct Queries {
    pool: mysql::Pool,
}

pub fn new(pool: mysql::Pool) -> Queries {
    return Queries { pool: pool };
}

impl Queries {
    pub fn test_rw(&self, now: u64) -> Result<(), QueriesError> {
        let pool = &self.pool.clone();

        // create table
        pool.prep_exec("CREATE TABLE IF NOT EXISTS dbpulse_rw (id INT NOT NULL, t INT(11) NOT NULL, PRIMARY KEY(id))", ())?;

        // write into table
        let mut stmt = pool
            .prepare("INSERT INTO dbpulse_rw (id, t) VALUES (1, ?) ON DUPLICATE KEY UPDATE t=?")?;
        stmt.execute((now, now))?;
        Ok(())
        /*
        pool.prep_exec("SELECT t FROM dbpulse_rw WHERE id=1", ())
            .map(|items| {
                for row in items {
                    match row {
                        Ok(row) => {
                            match mysql::from_row_opt::<u64>(row) {
                                Ok(rs) => {
                                    if now != rs {
                                        return Result::Err(Box::new(Error::NotMatching(
                                            "Oops".into(),
                                        )));
                                    }
                                }
                                Err(e) => {
                                    return Result::Err(Box::new(Error::MySQL(e.into())));
                                }
                            };
                        }
                        Err(e) => {
                            return Result::Err(Box::new(Error::MySQL(e)));
                        }
                    }
                }
                Ok(())
            })?;
        */
        //.ok();
        // Ok(());
    }
}
