use std::{error::Error, fmt};

#[derive(Debug)]
pub struct Queries {
    pool: mysql::Pool,
}

pub fn new(pool: mysql::Pool) -> Queries {
    return Queries { pool: pool };
}

impl Error for Queries {}

impl fmt::Display for Queries {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Oh no, something bad went down")
    }
}

impl Queries {
    pub fn test_rw(&self, now: u64) -> Result<(), mysql::Error> {
        let pool = &self.pool.clone();

        // create table
        pool.prep_exec("CREATE TABLE IF NOT EXISTS dbpulse_rw (id INT NOT NULL, t INT(11) NOT NULL, PRIMARY KEY(id))", ())?;

        // write into table
        let mut stmt = pool
            .prepare("INSERT INTO dbpulse_rw (id, t) VALUES (1, ?) ON DUPLICATE KEY UPDATE t=?")?;
        stmt.execute((now, now))?;

        pool.prep_exec("SELECT t FROM dbpulse_rw WHERE id=1", ())
            .map(|items| {
                for row in items {
                    match row {
                        Ok(row) => {
                            match mysql::from_row_opt::<u64>(row) {
                                Ok(rs) => {
                                    if now != rs {
                                        //return false);
                                    }
                                }
                                Err(e) => println!("{}", e),
                            };
                        }
                        Err(e) => println!("{}", e),
                    }
                }
            })?;

        Ok(())
    }
}
