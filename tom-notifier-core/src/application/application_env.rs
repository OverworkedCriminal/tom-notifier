use anyhow::anyhow;
use jsonwebtoken::{Algorithm, DecodingKey};
use std::{net::SocketAddr, str::FromStr, time::Duration};

pub struct ApplicationEnv {
    pub log_directory: String,
    pub log_filename: String,

    pub bind_address: SocketAddr,

    pub db_connection_string: String,
    pub db_name: String,

    pub max_notification_content_len: usize,
    pub max_http_content_len: usize,

    /// Algorithms must belong to the same family
    pub jwt_algorithms: Vec<Algorithm>,
    pub jwt_key: DecodingKey,

    pub rabbitmq_connection_string: String,
    pub rabbitmq_notifications_exchange_name: String,
    pub rabbitmq_confirmations_exchange_name: String,
    pub rabbitmq_confirmations_queue_name: String,
    pub rabbitmq_retry_interval: Duration,
}

impl ApplicationEnv {
    pub fn parse() -> anyhow::Result<Self> {
        let log_directory = std::env::var("TOM_NOTIFIER_CORE_LOG_DIRECTORY")?;
        let log_filename = std::env::var("TOM_NOTIFIER_CORE_LOG_FILENAME")?;
        let bind_address = std::env::var("TOM_NOTIFIER_CORE_BIND_ADDRESS")?.parse()?;
        let db_connection_string = std::env::var("TOM_NOTIFIER_CORE_DB_CONNECTION_STRING")?;
        let db_name = std::env::var("TOM_NOTIFIER_CORE_DB_NAME")?;
        let max_notification_content_len =
            std::env::var("TOM_NOTIFIER_CORE_MAX_NOTIFICATION_CONTENT_LEN")?.parse()?;
        let max_http_content_len =
            std::env::var("TOM_NOTIFIER_CORE_MAX_HTTP_CONTENT_LEN")?.parse()?;
        let jwt_algorithms =
            Self::parse_jwt_algorithms(std::env::var("TOM_NOTIFIER_CORE_JWT_ALGORITHMS")?)?;
        let jwt_algorithm = jwt_algorithms.first().ok_or(anyhow!(
            "TOM_NOTIFIER_CORE_JWT_ALGORITHMS need to contain at least one algorithm"
        ))?;
        let jwt_key =
            Self::parse_jwt_key(jwt_algorithm, std::env::var("TOM_NOTIFIER_CORE_JWT_KEY")?)?;
        let rabbitmq_connection_string =
            std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_CONNECTION_STRING")?;
        let rabbitmq_notifications_exchange_name =
            std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME")?;
        let rabbitmq_confirmations_exchange_name =
            std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_CONFIRMATIONS_EXCHANGE_NAME")?;
        let rabbitmq_confirmations_queue_name =
            std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_CONFIRMATIONS_QUEUE_NAME")?;
        let rabbitmq_retry_interval =
            std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_RETRY_INTERVAL")?.parse()?;
        let rabbitmq_retry_interval = Duration::from_secs(rabbitmq_retry_interval);

        Ok(Self {
            log_directory,
            log_filename,
            bind_address,
            db_connection_string,
            db_name,
            max_notification_content_len,
            max_http_content_len,
            jwt_algorithms,
            jwt_key,
            rabbitmq_connection_string,
            rabbitmq_notifications_exchange_name,
            rabbitmq_confirmations_exchange_name,
            rabbitmq_confirmations_queue_name,
            rabbitmq_retry_interval,
        })
    }

    fn parse_jwt_algorithms(jwt_algorithms: String) -> Result<Vec<Algorithm>, anyhow::Error> {
        let mut algorithms = Vec::new();

        for algorithm_str in jwt_algorithms.split(',') {
            let algorithm = Algorithm::from_str(algorithm_str)?;
            algorithms.push(algorithm);
        }

        match algorithms.is_empty() {
            true => Err(anyhow!("JWT_ALGORITHMS does not contain any algorithm")),
            false => Ok(algorithms),
        }
    }

    fn parse_jwt_key(
        jwt_algorithm: &Algorithm,
        jwt_key: String,
    ) -> Result<DecodingKey, anyhow::Error> {
        let jwt_key_bytes = jwt_key.as_bytes();

        let key = match jwt_algorithm {
            Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
                DecodingKey::from_secret(jwt_key_bytes)
            }
            Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem(jwt_key_bytes)?,
            Algorithm::RS256 | Algorithm::RS384 | Algorithm::RS512 => {
                DecodingKey::from_rsa_pem(jwt_key_bytes)?
            }
            Algorithm::PS256 | Algorithm::PS384 | Algorithm::PS512 => {
                DecodingKey::from_rsa_pem(jwt_key_bytes)?
            }
            Algorithm::EdDSA => DecodingKey::from_ec_pem(jwt_key_bytes)?,
        };

        Ok(key)
    }
}
