use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    dbpulse::cli::start().await
}
