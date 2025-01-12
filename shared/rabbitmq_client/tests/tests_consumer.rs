mod common;

use amqprs::{
    channel::{
        BasicConsumeArguments, BasicPublishArguments, ExchangeDeclareArguments, ExchangeType,
        QueueBindArguments, QueueDeclareArguments, QueueDeleteArguments,
    },
    BasicProperties,
};
use async_trait::async_trait;
use common::*;
use rabbitmq_client::consumer::{
    callback::{RabbitmqConsumerDeliveryCallback, RabbitmqConsumerStatusChangeCallback},
    error::ConsumeError,
    RabbitmqConsumer, RabbitmqConsumerStatus,
};
use serial_test::{parallel, serial};
use std::{process::Command, sync::Once, time::Duration};
use tokio::{
    sync::mpsc,
    time::{sleep, timeout},
};

static BEFORE_ALL: Once = Once::new();

#[tokio::test]
#[parallel]
async fn messages_received_by_the_consumer() {
    BEFORE_ALL.call_once(init_test_environment);

    const EXCHANGE: &str = "test messages_received_by_the_consumer";
    const QUEUE: &str = "test messages_received_by_the_consumer";

    let rabbitmq_connection = create_rabbitmq_connection().await;
    let exchange_declare_args = ExchangeDeclareArguments::of_type(EXCHANGE, ExchangeType::Direct);
    let queue_declare_args = QueueDeclareArguments::new(QUEUE)
        .exclusive(true)
        .auto_delete(true)
        .finish();
    let queue_bind_args = QueueBindArguments::new(QUEUE, EXCHANGE, "");
    let basic_consume_args = BasicConsumeArguments::new(QUEUE, "")
        .exclusive(true)
        .finish();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let consumer = Consumer { tx };
    let status_callback = MockStatusCallback;
    let rabbitmq_consumer = RabbitmqConsumer::new(
        rabbitmq_connection.clone(),
        exchange_declare_args,
        queue_declare_args,
        vec![queue_bind_args],
        basic_consume_args,
        consumer,
        status_callback,
    )
    .await
    .unwrap();

    let connection = create_connection().await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();

    let basic_properties = BasicProperties::default();
    let content = b"messages_received_by_the_consumer".to_vec();
    let args = BasicPublishArguments::new(EXCHANGE, "");
    channel
        .basic_publish(basic_properties, content.clone(), args)
        .await
        .unwrap();

    let message = timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(message, content);

    rabbitmq_consumer.close().await;
    rabbitmq_connection.close().await;

    channel.close().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
#[parallel]
async fn messages_received_by_the_consumer_after_consumer_cancellation() {
    BEFORE_ALL.call_once(init_test_environment);

    const EXCHANGE: &str = "test messages_received_by_the_consumer_after_consumer_cancellation";
    const QUEUE: &str = "test messages_received_by_the_consumer_after_consumer_cancellation";

    let rabbitmq_connection = create_rabbitmq_connection().await;
    let exchange_declare_args = ExchangeDeclareArguments::of_type(EXCHANGE, ExchangeType::Direct);
    let queue_declare_args = QueueDeclareArguments::new(QUEUE)
        .exclusive(false)
        .auto_delete(true)
        .finish();
    let queue_bind_args = QueueBindArguments::new(QUEUE, EXCHANGE, "");
    let basic_consume_args = BasicConsumeArguments::new(QUEUE, "")
        .consumer_tag("messages_received_by_the_consumer_after_consumer_cancellation".to_string())
        .exclusive(false)
        .finish();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let consumer = Consumer { tx };
    let status_callback = MockStatusCallback;
    let rabbitmq_consumer = RabbitmqConsumer::new(
        rabbitmq_connection.clone(),
        exchange_declare_args,
        queue_declare_args,
        vec![queue_bind_args],
        basic_consume_args,
        consumer,
        status_callback,
    )
    .await
    .unwrap();

    let connection = create_connection().await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();

    let basic_properties = BasicProperties::default();
    let content = b"messages_received_by_the_consumer_after_consumer_cancellation".to_vec();
    let args = BasicPublishArguments::new(EXCHANGE, "");
    channel
        .basic_publish(basic_properties, content.clone(), args)
        .await
        .unwrap();

    let message = timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(message, content);

    // Delete queue to force consumer cancelled signal
    let args = QueueDeleteArguments::new(QUEUE);
    channel.queue_delete(args).await.unwrap();

    // Sleep to make sure consumer has some time to recreate queue
    sleep(Duration::from_secs(5)).await;

    let basic_properties = BasicProperties::default();
    let content = b"messages_received_by_the_consumer_after_consumer_cancellation 2".to_vec();
    let args = BasicPublishArguments::new(EXCHANGE, "");
    channel
        .basic_publish(basic_properties, content.clone(), args)
        .await
        .unwrap();

    let message = timeout(Duration::from_secs(30), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(message, content);

    rabbitmq_consumer.close().await;
    rabbitmq_connection.close().await;

    channel.close().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
#[serial]
async fn messages_received_by_the_consumer_after_server_restart() {
    BEFORE_ALL.call_once(init_test_environment);

    const EXCHANGE: &str = "test messages_received_by_the_consumer_after_server_restart";
    const QUEUE: &str = "test messages_received_by_the_consumer_after_server_restart";

    let rabbitmq_connection = create_rabbitmq_connection().await;
    let exchange_declare_args = ExchangeDeclareArguments::of_type(EXCHANGE, ExchangeType::Direct);
    let queue_declare_args = QueueDeclareArguments::new(QUEUE)
        .exclusive(true)
        .auto_delete(true)
        .finish();
    let queue_bind_args = QueueBindArguments::new(QUEUE, EXCHANGE, "");
    let basic_consume_args = BasicConsumeArguments::new(QUEUE, "")
        .consumer_tag("messages_received_by_the_consumer_after_server_restart".to_string())
        .exclusive(true)
        .finish();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let consumer = Consumer { tx };
    let status_callback = MockStatusCallback;
    let rabbitmq_consumer = RabbitmqConsumer::new(
        rabbitmq_connection.clone(),
        exchange_declare_args,
        queue_declare_args,
        vec![queue_bind_args],
        basic_consume_args,
        consumer,
        status_callback,
    )
    .await
    .unwrap();

    let connection = create_connection().await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();

    let basic_properties = BasicProperties::default();
    let content = b"messages_received_by_the_consumer_after_server_restart".to_vec();
    let args = BasicPublishArguments::new(EXCHANGE, "");
    channel
        .basic_publish(basic_properties, content.clone(), args)
        .await
        .unwrap();

    let message = timeout(Duration::from_secs(10), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(message, content);

    // restart docker container to simulate network failure
    Command::new("docker")
        .arg("compose")
        .arg("restart")
        .arg("rabbitmq")
        .output()
        .unwrap();

    // Sleep to make sure consumer has some time to recreate everything
    sleep(Duration::from_secs(30)).await;

    let connection = create_connection().await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();

    let basic_properties = BasicProperties::default();
    let content = b"messages_received_by_the_consumer_after_server_restart 2".to_vec();
    let args = BasicPublishArguments::new(EXCHANGE, "");
    channel
        .basic_publish(basic_properties, content.clone(), args)
        .await
        .unwrap();

    let message = timeout(Duration::from_secs(90), rx.recv())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(message, content);

    rabbitmq_consumer.close().await;
    rabbitmq_connection.close().await;

    channel.close().await.unwrap();
    connection.close().await.unwrap();
}

#[tokio::test]
#[parallel]
async fn consumer_status_changes_when_connection_is_broken_and_recreated() {
    struct StatusCallback {
        tx: mpsc::UnboundedSender<RabbitmqConsumerStatus>,
    }
    #[async_trait]
    impl RabbitmqConsumerStatusChangeCallback for StatusCallback {
        async fn execute(&self, status: RabbitmqConsumerStatus) {
            self.tx.send(status).unwrap();
        }
    }

    BEFORE_ALL.call_once(init_test_environment);

    const EXCHANGE: &str = "test consumer_status_changes_when_connection_is_broken_and_recreated";
    const QUEUE: &str = "test consumer_status_changes_when_connection_is_broken_and_recreated";

    let rabbitmq_connection = create_rabbitmq_connection().await;
    let exchange_declare_args = ExchangeDeclareArguments::of_type(EXCHANGE, ExchangeType::Direct);
    let queue_declare_args = QueueDeclareArguments::new(QUEUE)
        .exclusive(false)
        .auto_delete(true)
        .finish();
    let queue_bind_args = QueueBindArguments::new(QUEUE, EXCHANGE, "");
    let basic_consume_args = BasicConsumeArguments::new(QUEUE, "")
        .consumer_tag("consumer_status_changes_when_connection_is_broken_and_recreated".to_string())
        .exclusive(false)
        .finish();
    let (tx, _rx) = mpsc::unbounded_channel();
    let consumer = Consumer { tx };
    let (consumer_status_tx, mut consumer_status_rx) = mpsc::unbounded_channel();
    let status_callback = StatusCallback {
        tx: consumer_status_tx,
    };
    let rabbitmq_consumer = RabbitmqConsumer::new(
        rabbitmq_connection.clone(),
        exchange_declare_args,
        queue_declare_args,
        vec![queue_bind_args],
        basic_consume_args,
        consumer,
        status_callback,
    )
    .await
    .unwrap();

    let connection = create_connection().await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();

    // Consuming is emitted on OK state enter
    // So it's necessary to read it here, so the rest of the test can proceed
    let status = timeout(Duration::from_secs(5), consumer_status_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(status, RabbitmqConsumerStatus::Consuming));

    // Delete queue to force consumer cancelled signal
    let args = QueueDeleteArguments::new(QUEUE);
    channel.queue_delete(args).await.unwrap();

    // Status change is expected after removing consumer's queue
    let status = timeout(Duration::from_secs(5), consumer_status_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(status, RabbitmqConsumerStatus::Recovering));

    // Status change after some time is expected after consumer
    // recreates everything
    let status = timeout(Duration::from_secs(5), consumer_status_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(status, RabbitmqConsumerStatus::Consuming));

    rabbitmq_consumer.close().await;
    rabbitmq_connection.close().await;

    channel.close().await.unwrap();
    connection.close().await.unwrap();
}

struct Consumer {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

#[async_trait]
impl RabbitmqConsumerDeliveryCallback for Consumer {
    async fn execute(&self, content: Vec<u8>) -> Result<(), ConsumeError> {
        self.tx.send(content).unwrap();
        Ok(())
    }
}

struct MockStatusCallback;
#[async_trait]
impl RabbitmqConsumerStatusChangeCallback for MockStatusCallback {
    async fn execute(&self, _status: RabbitmqConsumerStatus) {}
}
