use anyhow::anyhow;
use jsonwebtoken::{Algorithm, DecodingKey};
use jwt_auth::util::{parse_jwt_algorithms, parse_jwt_key};
use std::{net::SocketAddr, time::Duration};

pub struct ApplicationEnv {
    pub log_directory: String,
    pub log_filename: String,

    pub bind_address: SocketAddr,

    pub db_connection_string: String,
    pub db_name: String,

    pub websocket_ticket_lifespan: Duration,
    pub websocket_ping_interval: Duration,
    pub websocket_retry_max_count: u8,
    pub websocket_retry_interval: Duration,

    /// Algorithms must belong to the same family
    pub jwt_algorithms: Vec<Algorithm>,
    pub jwt_key: DecodingKey,

    pub rabbitmq_connection_string: String,
    pub rabbitmq_notifications_exchange_name: String,
    pub rabbitmq_notifications_queue_name: String,
    pub rabbitmq_confirmations_exchange_name: String,
    pub rabbitmq_retry_interval: Duration,
}

impl ApplicationEnv {
    pub fn parse() -> anyhow::Result<Self> {
        let log_directory = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_DIRECTORY")?;
        let log_filename = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_LOG_FILENAME")?;
        let bind_address = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_BIND_ADDRESS")?.parse()?;
        let db_connection_string = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_DB_CONNECTION_STRING")?;
        let db_name = Self::env_var("TOM_NOTIFIER_WS_DELIVERY_DB_NAME")?;
        let websocket_ticket_lifespan =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_WEBSOCKET_TICKET_LIFESPAN")?.parse()?;
        let websocket_ping_interval =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_WEBSOCKET_PING_INTERVAL")?.parse()?;
        let websocket_ping_interval = Duration::from_secs(websocket_ping_interval);
        let websocket_retry_max_count =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_WEBSOCKET_RETRY_MAX_COUNT")?.parse()?;
        let websocket_retry_interval =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_WEBSOCKET_RETRY_INTERVAL")?.parse()?;
        let websocket_retry_interval = Duration::from_secs(websocket_retry_interval);
        let websocket_ticket_lifespan = Duration::from_secs(websocket_ticket_lifespan);
        let jwt_algorithms =
            parse_jwt_algorithms(Self::env_var("TOM_NOTIFIER_WS_DELIVERY_JWT_ALGORITHMS")?)?;
        let jwt_algorithm = jwt_algorithms.first().ok_or(anyhow!(
            "TOM_NOTIFIER_WS_DELIVERY_JWT_ALGORITHMS need to contain at least one algorithm"
        ))?;
        let jwt_key = parse_jwt_key(
            jwt_algorithm,
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_JWT_KEY")?,
        )?;
        let rabbitmq_connection_string =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_CONNECTION_STRING")?;
        let rabbitmq_notifications_exchange_name =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME")?;
        let rabbitmq_notifications_queue_name =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_NOTIFICATIONS_QUEUE_NAME")?;
        let rabbitmq_confirmations_exchange_name =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_CONFIRMATIONS_EXCHANGE_NAME")?;
        let rabbitmq_retry_interval =
            Self::env_var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_RETRY_INTERVAL")?.parse()?;
        let rabbitmq_retry_interval = Duration::from_secs(rabbitmq_retry_interval);

        Ok(Self {
            log_directory,
            log_filename,
            bind_address,
            db_connection_string,
            db_name,
            websocket_ticket_lifespan,
            websocket_ping_interval,
            websocket_retry_max_count,
            websocket_retry_interval,
            jwt_algorithms,
            jwt_key,
            rabbitmq_connection_string,
            rabbitmq_notifications_exchange_name,
            rabbitmq_notifications_queue_name,
            rabbitmq_confirmations_exchange_name,
            rabbitmq_retry_interval,
        })
    }

    fn env_var(name: &'static str) -> anyhow::Result<String> {
        std::env::var(name).map_err(|_| anyhow!("environment variable {name} not set"))
    }
}
