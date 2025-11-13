mod run;

use crate::tls::TlsConfig;
use dsn::DSN;
use std::net::IpAddr;

/// Action enum representing each possible command
#[derive(Debug)]
pub enum Action {
    Monitor {
        dsn: DSN,
        interval: u16,
        listen: Option<IpAddr>,
        port: u16,
        range: u32,
        tls: TlsConfig,
    },
}

impl Action {
    /// Execute the action
    ///
    /// # Errors
    ///
    /// Returns an error if the action fails to execute
    pub async fn execute(self) -> anyhow::Result<()> {
        run::execute(self).await
    }
}
