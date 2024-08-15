use amqprs::connection::{Connection, OpenConnectionArguments};
use rabbitmq_client::{RabbitmqConnection, RabbitmqConnectionConfig};
use std::time::Duration;
use tracing::level_filters::LevelFilter;

pub fn init_test_environment() {
    // read envs from .env file
    dotenvy::dotenv().unwrap();

    // setup tracing
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .with_target(false)
        .with_test_writer()
        .init();
}

pub async fn create_connection() -> Result<Connection, amqprs::error::Error> {
    let rabbitmq_connection_uri = std::env::var("TEST_RABBITMQ_CONNECTION_URI").unwrap();
    let args = OpenConnectionArguments::try_from(rabbitmq_connection_uri.as_str()).unwrap();

    Connection::open(&args).await
}

pub async fn create_rabbitmq_connection() -> RabbitmqConnection {
    let rabbitmq_connection_uri = std::env::var("TEST_RABBITMQ_CONNECTION_URI").unwrap();
    let retry_interval = std::env::var("TEST_RETRY_INTERVAL")
        .unwrap()
        .parse()
        .unwrap();

    let config = RabbitmqConnectionConfig {
        retry_interval: Duration::from_secs(retry_interval),
    };
    let args = OpenConnectionArguments::try_from(rabbitmq_connection_uri.as_str()).unwrap();
    let connection = RabbitmqConnection::new(config, args).await.unwrap();

    connection
}
