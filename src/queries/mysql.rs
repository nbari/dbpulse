use anyhow::{anyhow, Context, Result};
use chrono::prelude::*;
use chrono::{DateTime, Utc};
use dsn::DSN;
use rand::Rng;
use sqlx::{mysql::MySqlConnectOptions, ConnectOptions, Connection, Executor};
use uuid::Uuid;

pub async fn test_rw(dsn: &DSN, now: DateTime<Utc>) -> Result<String> {
    let mut options = MySqlConnectOptions::new()
        .username(dsn.username.clone().unwrap_or_default().as_ref())
        .password(dsn.password.clone().unwrap_or_default().as_str())
        .database(dsn.database.clone().unwrap_or_default().as_ref());

    if let Some(host) = &dsn.host {
        options = options.host(host.as_str()).port(dsn.port.unwrap_or(3306));
    } else if let Some(socket) = &dsn.socket {
        options = options.socket(socket.as_str());
    }

    let mut conn = options.connect().await?;

    // create table
    let create_table_sql = r#"
        CREATE TABLE IF NOT EXISTS dbpulse_rw (
            id INT NOT NULL,
            t1 INT(11) NOT NULL ,
            t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            uuid CHAR(36) CHARACTER SET ascii,
            UNIQUE KEY(uuid),
            PRIMARY KEY(id)
        ) ENGINE=InnoDB
    "#;

    conn.execute(create_table_sql).await?;

    // write into table
    let id: u32 = rand::thread_rng().gen_range(0..100);
    let uuid = Uuid::new_v4();

    // SQL Query
    sqlx::query(
        r#"
        INSERT INTO dbpulse_rw (id, t1, uuid)
        VALUES (?, ?, ?)
        ON DUPLICATE KEY UPDATE
        t1 = VALUES(t1), uuid = VALUES(uuid)
        "#,
    )
    .bind(id)
    .bind(now.timestamp())
    .bind(uuid.to_string())
    .execute(&mut conn)
    .await?;

    // Check if stored record matches
    let row: Option<(i64, String)> = sqlx::query_as(
        r#"
        SELECT t1, uuid
        FROM dbpulse_rw
        WHERE id = ?
        "#,
    )
    .bind(id)
    .fetch_optional(&mut conn)
    .await
    .context("Failed to query the database")?;

    // Ensure the row exists and matches
    let (t1, v4) = row.context("Expected records")?;
    if now.timestamp() != t1 || uuid.to_string() != v4 {
        return Err(anyhow!(
            "Records don't match: expected ({}, {}), got ({}, {})",
            now.timestamp(),
            uuid,
            t1,
            v4
        ));
    }

    // Start a transaction to set all `t1` records to 0
    let mut tx = conn.begin().await?;
    sqlx::query("UPDATE dbpulse_rw SET t1 = ?")
        .bind(0)
        .execute(tx.as_mut())
        .await?;
    let rows: Vec<i64> = sqlx::query_scalar("SELECT t1 FROM dbpulse_rw")
        .fetch_all(tx.as_mut())
        .await
        .context("Failed to fetch rows")?;

    for row in rows {
        if row != 0 {
            return Err(anyhow!("Records don't match: {} != {}", row, 0));
        }
    }

    // Roll back this transaction
    tx.rollback().await?;

    // Start a new transaction to update record 0 with current timestamp
    let mut tx = conn.begin().await?;
    sqlx::query(
        r#"
        INSERT INTO dbpulse_rw (id, t1, uuid)
        VALUES (0, ?, UUID())
        ON DUPLICATE KEY UPDATE t1 = ?
        "#,
    )
    .bind(now.timestamp())
    .bind(now.timestamp())
    .execute(tx.as_mut())
    .await
    .context("Failed to insert or update record")?;
    tx.commit().await?;

    // Drop the table conditionally
    if now.minute() == id {
        sqlx::query("DROP TABLE dbpulse_rw")
            .execute(&mut conn)
            .await
            .context("Failed to drop table")?;
    }

    // Get database version
    let version: Option<String> = sqlx::query_scalar("SELECT VERSION()")
        .fetch_optional(&mut conn)
        .await
        .context("Failed to fetch database version")?;
    drop(conn);

    version.context("Expected database version")
}
