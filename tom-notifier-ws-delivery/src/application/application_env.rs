use anyhow::anyhow;
use std::net::SocketAddr;

pub struct ApplicationEnv {
    pub log_directory: String,
    pub log_filename: String,

    pub bind_address: SocketAddr,
}

impl ApplicationEnv {
    pub fn parse() -> anyhow::Result<Self> {
        let log_directory = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_DIRECTORY")?;
        let log_filename = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_FILENAME")?;

        let bind_address = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_BIND_ADDRESS")?.parse()?;

        Ok(Self {
            log_directory,
            log_filename,
            bind_address,
        })
    }

    fn env_var(name: &'static str) -> anyhow::Result<String> {
        std::env::var(name).map_err(|_| anyhow!("environment variable {name} not set"))
    }
}
