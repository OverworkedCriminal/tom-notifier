use anyhow::anyhow;

pub struct ApplicationEnv {
    pub log_directory: String,
    pub log_filename: String,
}

impl ApplicationEnv {
    pub fn parse() -> anyhow::Result<Self> {
        let log_directory = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_DIRECTORY")?;
        let log_filename = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_FILENAME")?;

        Ok(Self {
            log_directory,
            log_filename,
        })
    }

    fn env_var(name: &'static str) -> anyhow::Result<String> {
        std::env::var(name).map_err(|_| anyhow!("environment variable {name} not set"))
    }
}
