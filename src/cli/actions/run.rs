use super::Action;

/// Execute the action's business logic by delegating to the appropriate module
pub async fn execute(action: Action) -> anyhow::Result<()> {
    match action {
        Action::Monitor {
            dsn,
            interval,
            listen,
            port,
            range,
            tls,
        } => crate::pulse::start(dsn, interval, listen, port, range, tls).await,
    }
}
