use anyhow::{anyhow, Context, Result};
use chrono::prelude::*;
use chrono::{DateTime, Utc};
use dsn::DSN;
use rand::Rng;
use sqlx::{
    postgres::{PgConnectOptions, PgDatabaseError},
    ConnectOptions, Connection,
};
use uuid::Uuid;

pub async fn test_rw(dsn: &DSN, now: DateTime<Utc>, range: u32) -> Result<String> {
    let mut options = PgConnectOptions::new()
        .username(dsn.username.clone().unwrap_or_default().as_ref())
        .password(dsn.password.clone().unwrap_or_default().as_str())
        .database(dsn.database.clone().unwrap_or_default().as_ref());

    if let Some(host) = &dsn.host {
        options = options.host(host.as_str()).port(dsn.port.unwrap_or(5432));
    } else if let Some(socket) = &dsn.socket {
        options = options.socket(socket.as_str());
    }

    let mut conn = match options.connect().await {
        Ok(conn) => conn,
        Err(err) => match err {
            sqlx::Error::Database(db_err) => {
                if db_err
                    .as_error()
                    .downcast_ref::<PgDatabaseError>()
                    .map(PgDatabaseError::code)
                    == Some("3D000")
                {
                    let tmp_options = options.clone().database("postgres");
                    let mut tmp_conn = tmp_options.connect().await?;
                    sqlx::query(&format!(
                        "CREATE DATABASE {}",
                        dsn.database.clone().unwrap_or_default()
                    ))
                    .execute(&mut tmp_conn)
                    .await?;
                    drop(tmp_conn);
                    options.connect().await?
                } else {
                    return Err(db_err.into());
                }
            }
            _ => return Err(err.into()),
        },
    };

    // Get database version
    let version: Option<String> = sqlx::query_scalar("SHOW server_version")
        .fetch_optional(&mut conn)
        .await
        .context("Failed to fetch database version")?;

    // Query to check if the database is in recovery (read-only)
    let is_in_recovery: (bool,) = sqlx::query_as("SELECT pg_is_in_recovery();")
        .fetch_one(&mut conn)
        .await?;

    // can't write to a read-only database
    if is_in_recovery.0 {
        return Ok(format!(
            "{} - Database is in recovery mode",
            version.unwrap_or_default()
        ));
    }

    // for UUID
    sqlx::query("CREATE EXTENSION IF NOT EXISTS \"uuid-ossp\"")
        .execute(&mut conn)
        .await?;

    // create table
    let create_table_sql = r#"
        CREATE TABLE IF NOT EXISTS dbpulse_rw (
            id SERIAL PRIMARY KEY,
            t1 BIGINT NOT NULL,
            t2 TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP,
            uuid UUID NOT NULL,
            CONSTRAINT uuid_unique UNIQUE (uuid)
        )
    "#;

    sqlx::query(create_table_sql).execute(&mut conn).await?;

    // write into table
    let id: u32 = rand::thread_rng().gen_range(0..range);
    let uuid = Uuid::new_v4();

    // SQL Query
    sqlx::query(
        r#"
        INSERT INTO dbpulse_rw (id, t1, uuid)
        VALUES ($1, $2, $3)
        ON CONFLICT (id)
        DO UPDATE SET t1 = EXCLUDED.t1, uuid = EXCLUDED.uuid
        "#,
    )
    .bind(id as i32)
    .bind(now.timestamp())
    .bind(uuid)
    .execute(&mut conn) // Ensure we're using PgConnection here
    .await?;

    // Check if stored record matches
    let row: Option<(i64, Uuid)> = sqlx::query_as(
        r#"
        SELECT t1, uuid
        FROM dbpulse_rw
        WHERE id = $1
        "#,
    )
    .bind(id as i32)
    .fetch_optional(&mut conn)
    .await?;

    // Ensure the row exists and matches
    let (t1, v4) = row.context("Expected records")?;
    if now.timestamp() != t1 || uuid != v4 {
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
    sqlx::query("UPDATE dbpulse_rw SET t1 = $1")
        .bind(0)
        .execute(tx.as_mut())
        .await?;
    let rows: Vec<i64> = sqlx::query_scalar("SELECT t1 FROM dbpulse_rw")
        .fetch_all(tx.as_mut())
        .await?;

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
        VALUES (0, $1, UUID_GENERATE_V4())
        ON CONFLICT (id)
        DO UPDATE SET t1 = EXCLUDED.t1
        "#,
    )
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

    drop(conn);

    version.context("Expected database version")
}
