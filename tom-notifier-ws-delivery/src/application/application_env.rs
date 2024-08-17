use anyhow::anyhow;
use jsonwebtoken::{Algorithm, DecodingKey};
use jwt_auth::util::{parse_jwt_algorithms, parse_jwt_key};
use std::net::SocketAddr;

pub struct ApplicationEnv {
    pub log_directory: String,
    pub log_filename: String,

    pub bind_address: SocketAddr,

    /// Algorithms must belong to the same family
    pub jwt_algorithms: Vec<Algorithm>,
    pub jwt_key: DecodingKey,
}

impl ApplicationEnv {
    pub fn parse() -> anyhow::Result<Self> {
        let log_directory = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_DIRECTORY")?;
        let log_filename = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_FILENAME")?;
        let bind_address = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_BIND_ADDRESS")?.parse()?;
        let jwt_algorithms =
            parse_jwt_algorithms(Self::env_var("TOM_NOTIFIER_WS_DELIVERY_JWT_ALGORITHMS")?)?;
        let jwt_algorithm = jwt_algorithms.first().ok_or(anyhow!(
            "TOM_NOTIFIER_WS_DELIVERY_JWT_ALGORITHMS need to contain at least one algorithm"
        ))?;
        let jwt_key = parse_jwt_key(
            jwt_algorithm,
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_JWT_KEY")?,
        )?;

        Ok(Self {
            log_directory,
            log_filename,
            bind_address,
            jwt_algorithms,
            jwt_key,
        })
    }

    fn env_var(name: &'static str) -> anyhow::Result<String> {
        std::env::var(name).map_err(|_| anyhow!("environment variable {name} not set"))
    }
}
