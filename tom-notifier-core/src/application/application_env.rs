pub struct ApplicationEnv {
    pub log_directory: String,
    pub log_filename: String,
}

impl ApplicationEnv {
    pub fn parse() -> anyhow::Result<Self> {
        let log_directory = std::env::var("TOM_NOTIFIER_CORE_LOG_DIRECTORY")?;
        let log_filename = std::env::var("TOM_NOTIFIER_CORE_LOG_FILENAME")?;

        Ok(Self {
            log_directory,
            log_filename,
        })
    }
}
