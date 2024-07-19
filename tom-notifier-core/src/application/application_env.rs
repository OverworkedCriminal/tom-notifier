use std::net::SocketAddr;

pub struct ApplicationEnv {
    pub log_directory: String,
    pub log_filename: String,
    pub bind_address: SocketAddr,
}

impl ApplicationEnv {
    pub fn parse() -> anyhow::Result<Self> {
        let log_directory = std::env::var("TOM_NOTIFIER_CORE_LOG_DIRECTORY")?;
        let log_filename = std::env::var("TOM_NOTIFIER_CORE_LOG_FILENAME")?;
        let bind_address = std::env::var("TOM_NOTIFIER_CORE_BIND_ADDRESS")?.parse()?;

        Ok(Self {
            log_directory,
            log_filename,
            bind_address,
        })
    }
}
