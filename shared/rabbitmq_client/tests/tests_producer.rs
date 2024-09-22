mod common;

use amqprs::{
    channel::{
        BasicGetArguments, ExchangeDeclareArguments, ExchangeType, QueueBindArguments,
        QueueDeclareArguments, QueueDeleteArguments,
    },
    BasicProperties,
};
use common::*;
use rabbitmq_client::producer::RabbitmqProducer;
use serial_test::serial;
use std::{process::Command, sync::Once, time::Duration};
use time::OffsetDateTime;
use tokio::time::{sleep, timeout};

static BEFORE_ALL: Once = Once::new();

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
