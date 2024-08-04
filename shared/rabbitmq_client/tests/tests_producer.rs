use amqprs::{
    channel::{
        BasicGetArguments, ExchangeDeclareArguments, ExchangeType, QueueBindArguments,
        QueueDeclareArguments, QueueDeleteArguments,
    },
    connection::{Connection, OpenConnectionArguments},
    BasicProperties,
};
use rabbitmq_client::{RabbitmqConnection, RabbitmqConnectionConfig, RabbitmqProducer};
use serial_test::serial;
use std::{process::Command, sync::Once, time::Duration};
use time::OffsetDateTime;
use tokio::time::{sleep, timeout};
use tracing::level_filters::LevelFilter;

static BEFORE_ALL: Once = Once::new();

fn init_test_environment() {
    // read envs from .env file
    dotenvy::dotenv().unwrap();

    // setup tracing
    tracing_subscriber::fmt()
        .with_max_level(LevelFilter::TRACE)
        .with_target(false)
        .with_test_writer()
        .init();
}

async fn create_connection() -> Result<Connection, amqprs::error::Error> {
    let rabbitmq_connection_uri = std::env::var("TEST_RABBITMQ_CONNECTION_URI").unwrap();
    let args = OpenConnectionArguments::try_from(rabbitmq_connection_uri.as_str()).unwrap();

    Connection::open(&args).await
}

async fn create_rabbitmq_connection() -> RabbitmqConnection {
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

#[tokio::test]
#[serial]
async fn produced_messages_received_by_the_consumer() {
    BEFORE_ALL.call_once(init_test_environment);

    let now = OffsetDateTime::now_utc();
    let exchange_name =
        "test rabbitmq_client produced_messages_received_by_the_consumer".to_string();
    let queue_name =
        format!("test rabbitmq_client produced_messages_received_by_the_consumer {now}");
    let routing_key = "produced_messages_received_by_the_consumer".to_string();

    let rabbitmq_connection = create_rabbitmq_connection().await;
    let exchange_declare_args =
        ExchangeDeclareArguments::of_type(&exchange_name, ExchangeType::Direct);
    let rabbitmq_producer =
        RabbitmqProducer::new(rabbitmq_connection.clone(), exchange_declare_args)
            .await
            .unwrap();

    let content = b"produced_messages_received_by_the_consumer".to_vec();

    let connection = create_connection().await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();
    channel
        .queue_declare(QueueDeclareArguments::new(&queue_name))
        .await
        .unwrap();
    channel
        .queue_bind(QueueBindArguments::new(
            &queue_name,
            &exchange_name,
            &routing_key,
        ))
        .await
        .unwrap();

    rabbitmq_producer.send(routing_key, BasicProperties::default(), content.clone());

    let (_get_ok, _properties, received_content) = timeout(Duration::from_secs(5), async {
        loop {
            match channel
                .basic_get(BasicGetArguments::new(&queue_name))
                .await
                .unwrap()
            {
                Some(message) => return message,
                None => sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(received_content, content);

    channel
        .queue_delete(QueueDeleteArguments::new(&queue_name))
        .await
        .unwrap();

    channel.close().await.unwrap();
    connection.close().await.unwrap();

    rabbitmq_producer.close().await;
    rabbitmq_connection.close().await;
}

#[tokio::test]
#[serial]
async fn produced_messages_received_by_the_consumer_after_connection_failure() {
    BEFORE_ALL.call_once(init_test_environment);

    let now = OffsetDateTime::now_utc();
    let exchange_name =
        "test rabbitmq_client produced_messages_received_by_the_consumer_after_connection_failure"
            .to_string();
    let queue_name =
        format!("test rabbitmq_client produced_messages_received_by_the_consumer_after_connection_failure {now}");
    let routing_key =
        "produced_messages_received_by_the_consumer_after_connection_failure".to_string();

    let rabbitmq_connection = create_rabbitmq_connection().await;
    // Since this test causes rabbitmq restart it is necessary
    // to mark exchange as durable
    let exchange_declare_args =
        ExchangeDeclareArguments::of_type(&exchange_name, ExchangeType::Direct)
            .durable(true)
            .finish();
    let rabbitmq_producer =
        RabbitmqProducer::new(rabbitmq_connection.clone(), exchange_declare_args)
            .await
            .unwrap();

    let content = b"produced_messages_received_by_the_consumer_after_connection_failure".to_vec();

    let connection = create_connection().await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();
    channel
        .queue_declare(
            // Since this test causes rabbitmq restart it is necessary
            // to mark queue as durable
            QueueDeclareArguments::new(&queue_name)
                .durable(true)
                .finish(),
        )
        .await
        .unwrap();
    channel
        .queue_bind(QueueBindArguments::new(
            &queue_name,
            &exchange_name,
            &routing_key,
        ))
        .await
        .unwrap();

    // restart docker container to simulate network failure
    Command::new("docker")
        .arg("compose")
        .arg("restart")
        .arg("rabbitmq")
        .output()
        .unwrap();

    rabbitmq_producer.send(routing_key, BasicProperties::default(), content.clone());

    let connection = timeout(Duration::from_secs(60), async {
        loop {
            match create_connection().await {
                Ok(connection) => return connection,
                Err(_) => sleep(Duration::from_millis(500)).await,
            }
        }
    })
    .await
    .unwrap();
    let channel = connection.open_channel(None).await.unwrap();

    let (_get_ok, _properties, received_content) = timeout(Duration::from_secs(5), async {
        loop {
            match channel
                .basic_get(BasicGetArguments::new(&queue_name))
                .await
                .unwrap()
            {
                Some(message) => return message,
                None => sleep(Duration::from_millis(100)).await,
            }
        }
    })
    .await
    .unwrap();

    assert_eq!(received_content, content);

    channel
        .queue_delete(QueueDeleteArguments::new(&queue_name))
        .await
        .unwrap();

    channel.close().await.unwrap();
    connection.close().await.unwrap();

    rabbitmq_producer.close().await;
    rabbitmq_connection.close().await;
}
