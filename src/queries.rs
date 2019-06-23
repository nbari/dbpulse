use std::{error, time::SystemTime};

pub struct Queries {
    pool: mysql::Pool,
}

pub fn new(pool: mysql::Pool) -> Queries {
    return Queries { pool: pool };
}

impl Queries {
    pub fn test_rw(&self) -> Result<bool, Box<error::Error>> {
        let pool = &self.pool.clone();
        let n = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        let now = n.as_secs();

        // create table
        &self.pool.prep_exec("CREATE TABLE IF NOT EXISTS dbpulse_rw (id INT NOT NULL, t INT(11) NOT NULL, PRIMARY KEY(id))", ())?;

        // write into table
        let mut stmt = pool
            .prepare("INSERT INTO dbpulse_rw (id, t) VALUES (1, ?) ON DUPLICATE KEY UPDATE t=?")?;
        stmt.execute((now, now))?;

        pool.prep_exec("SELECT t FROM dbpulse_rw WHERE id=1", ())
            .map(|items| {
                for row in items {
                    match row {
                        Ok(row) => {
                            let rs = mysql::from_row::<u64>(row);
                            if now != rs {
                                let pool = pool.clone();
                                //                                send_msg(pool);
                            }
                        }
                        Err(e) => println!("{}", e),
                    }
                }
            })?;

        Ok(true)
    }
}
