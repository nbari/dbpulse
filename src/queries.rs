use anyhow::{anyhow, Context, Result};
use chrono::prelude::*;
use chrono::{DateTime, Utc};
use mysql_async::prelude::*;
use rand::Rng;
use uuid::Uuid;

pub async fn test_rw(opts: mysql_async::OptsBuilder, now: DateTime<Utc>) -> Result<String> {
    let mut conn = mysql_async::Conn::new(opts).await?;

    // create table
    r#"CREATE TABLE IF NOT EXISTS dbpulse_rw (
        id INT NOT NULL,
        t1 INT(11) NOT NULL ,
        t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
        uuid CHAR(36) CHARACTER SET ascii,
        UNIQUE KEY(uuid),
        PRIMARY KEY(id)
    ) ENGINE=InnoDB"#
        .ignore(&mut conn)
        .await?;

    // write into table
    let num: u32 = rand::thread_rng().gen_range(0, 100);
    let uuid = Uuid::new_v4();
    conn.exec_drop("INSERT INTO dbpulse_rw (id, t1, uuid) VALUES (:id, :t1, :uuid) ON DUPLICATE KEY UPDATE t1=:t1, uuid=:uuid", params! {
        "id" => num,
        "t1" => now.timestamp(),
        "uuid" => uuid.to_string(),
    }).await?;

    // check if stored record matches
    let row: Option<(i64, String)> = conn
        .exec_first(
            "SELECT t1, uuid FROM dbpulse_rw Where id=:id",
            params! {
                    "id" => num,
            },
        )
        .await?;

    let (t1, v4) = row.context("Expected records")?;
    if now.timestamp() != t1 || uuid.to_string() != v4 {
        return Err(anyhow!(
            "Records don't match: {}",
            format!("({}, {}) != ({},{})", now, uuid, t1, v4)
        ));
    }

    // check transaction setting all records to 0
    let mut tx = conn
        .start_transaction(mysql_async::TxOpts::default())
        .await?;
    tx.exec_drop(
        "UPDATE dbpulse_rw SET t1=:t1",
        params! {
            "t1" => "0"
        },
    )
    .await?;
    let rows = tx.exec("SELECT t1 FROM dbpulse_rw", ()).await?;
    for row in rows {
        let row = mysql_async::from_row::<u64>(row);
        if row != 0 {
            return Err(anyhow!(
                "Records don't match: {}",
                format!("{} != {}", row, 0)
            ));
        }
    }
    tx.rollback().await?;

    // update record 1 with now
    let mut tx = conn
        .start_transaction(mysql_async::TxOpts::default())
        .await?;
    tx.exec_drop(
            "INSERT INTO dbpulse_rw (id, t1, uuid) VALUES (0, :t1, UUID()) ON DUPLICATE KEY UPDATE t1=:t1",
            params!{
                "t1" => now.timestamp()
            },
        ).await?;
    tx.commit().await?;

    // drop table
    if now.minute() == num {
        conn.query_drop("DROP TABLE dbpulse_rw").await?;
    }

    // get db version
    let version: Option<String> = conn.query_first("SELECT VERSION()").await?;
    drop(conn);

    Ok(version.context("Expected version")?)
}
