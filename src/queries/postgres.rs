use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use dsn::DSN;

pub async fn test_rw(dsn: &DSN, now: DateTime<Utc>) -> Result<String> {
    todo!();

    Ok("".to_string())
}
