mod common;
pub use common::*;

use amqprs::{
    channel::{
        BasicGetArguments, Channel, QueueBindArguments, QueueDeclareArguments, QueueDeleteArguments,
    },
    connection::{Connection, OpenConnectionArguments},
};
use base64::{prelude::BASE64_STANDARD, Engine};
use prost::Message;
use protobuf::notification::{NotificationProtobuf, NotificationStatusProtobuf};
use reqwest::{header::CONTENT_TYPE, Client, StatusCode};
use serde_json::{json, Value};
use std::{str::FromStr, time::Duration};
use time::OffsetDateTime;
use tokio::time::timeout;
use uuid::Uuid;

mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/protobuf.rs"));
}

#[tokio::test]
async fn new_notification() {
    // after producing notification
    // fetching notification from queue bound to 'NEW' routing key
    // should return produced notification

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let producer = create_producer_jwt();

    let queue = format!("test core new_notification {}", OffsetDateTime::now_utc());
    let (connection, channel) = init_rabbitmq(&queue, "NEW").await;

    let content_type = "ascii";
    let content = b"there's some of my content".to_vec();

    let time_before_send = OffsetDateTime::now_utc();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": None as Option<OffsetDateTime>,
                "user_ids": [user_id],
                "producer_notification_id": 1,
                "content_type": content_type,
                "content": BASE64_STANDARD.encode(&content),
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let notification = serde_json::from_slice::<Value>(&response_body).unwrap();
    let id = notification.get("id").unwrap().as_str().unwrap();

    let (_get_ok, _basic_properties, bytes) = timeout(Duration::from_secs(5), async {
        let mut message = None;
        while message.is_none() {
            let args = BasicGetArguments::new(&queue);
            message = channel.basic_get(args).await.unwrap();
        }
        message.unwrap()
    })
    .await
    .unwrap();

    let time_now = OffsetDateTime::now_utc();

    let notification = NotificationProtobuf::decode(bytes.as_slice()).unwrap();
    assert_eq!(notification.user_ids.len(), 1);
    let notification_user_id_str = notification.user_ids.first().unwrap();
    let notification_user_id = Uuid::from_str(notification_user_id_str).unwrap();
    assert_eq!(notification_user_id, user_id);
    assert_eq!(notification.id, id);
    assert_eq!(notification.status(), NotificationStatusProtobuf::New);
    let notification_timestamp = notification.timestamp.unwrap();
    let notification_datetime = OffsetDateTime::from_unix_timestamp(notification_timestamp.seconds)
        .unwrap()
        .replace_nanosecond(notification_timestamp.nanos as u32)
        .unwrap();
    assert!(time_before_send <= notification_datetime && notification_datetime <= time_now);
    assert_eq!(notification.seen.unwrap(), false);
    assert_eq!(notification.content_type.unwrap(), content_type);
    assert_eq!(notification.content.unwrap(), content);

    destroy_rabbitmq(connection, channel, &queue).await;
}

#[tokio::test]
async fn updated_notification() {
    // after producing notification,
    // marking it as delivered
    // and updating seen
    // fetching notification from queue bound to 'UPDATED' routing key
    // should return produced notification

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let user = create_consumer_jwt_with_id(user_id);
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": None as Option<OffsetDateTime>,
                "user_ids": [user_id],
                "producer_notification_id": 1,
                "content_type": "ascii",
                "content": BASE64_STANDARD.encode(b"there's some of my content"),
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let notification = serde_json::from_slice::<Value>(&response_body).unwrap();
    let id = notification.get("id").unwrap().as_str().unwrap();

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let queue = format!(
        "test core updated_notification {}",
        OffsetDateTime::now_utc()
    );
    let (connection, channel) = init_rabbitmq(&queue, "UPDATED").await;

    let time_before_send = OffsetDateTime::now_utc();

    let seen = true;

    let response = client
        .put(format!(
            "http://{}/api/v1/notifications/delivered/{id}/seen",
            address(),
        ))
        .bearer_auth(&user)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json! ({
                "seen": seen,
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (_get_ok, _basic_properties, bytes) = timeout(Duration::from_secs(5), async {
        let mut message = None;
        while message.is_none() {
            let args = BasicGetArguments::new(&queue);
            message = channel.basic_get(args).await.unwrap();
        }
        message.unwrap()
    })
    .await
    .unwrap();

    let time_now = OffsetDateTime::now_utc();

    let notification = NotificationProtobuf::decode(bytes.as_slice()).unwrap();
    assert_eq!(notification.user_ids.len(), 1);
    let notification_user_id_str = notification.user_ids.first().unwrap();
    let notification_user_id = Uuid::from_str(notification_user_id_str).unwrap();
    assert_eq!(notification_user_id, user_id);
    assert_eq!(notification.id, id);
    assert_eq!(notification.status(), NotificationStatusProtobuf::Updated);
    let notification_timestamp = notification.timestamp.unwrap();
    let notification_datetime = OffsetDateTime::from_unix_timestamp(notification_timestamp.seconds)
        .unwrap()
        .replace_nanosecond(notification_timestamp.nanos as u32)
        .unwrap();
    assert!(time_before_send <= notification_datetime && notification_datetime <= time_now);
    assert_eq!(notification.seen.unwrap(), seen);
    assert!(notification.content_type.is_none());
    assert!(notification.content.is_none());

    destroy_rabbitmq(connection, channel, &queue).await;
}

#[tokio::test]
async fn deleted_notification() {
    // after producing notification,
    // marking it as delivered
    // and deleting it
    // fetching notification from queue bound to 'DELETED' routing key
    // should return produced notification

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let user = create_consumer_jwt_with_id(user_id);
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": None as Option<OffsetDateTime>,
                "user_ids": [user_id],
                "producer_notification_id": 1,
                "content_type": "ascii",
                "content": BASE64_STANDARD.encode(b"there's some of my content"),
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let notification = serde_json::from_slice::<Value>(&response_body).unwrap();
    let id = notification.get("id").unwrap().as_str().unwrap();

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let queue = format!(
        "test core updated_notification {}",
        OffsetDateTime::now_utc()
    );
    let (connection, channel) = init_rabbitmq(&queue, "DELETED").await;

    let time_before_send = OffsetDateTime::now_utc();

    let response = client
        .delete(format!(
            "http://{}/api/v1/notifications/delivered/{id}",
            address(),
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let (_get_ok, _basic_properties, bytes) = timeout(Duration::from_secs(5), async {
        let mut message = None;
        while message.is_none() {
            let args = BasicGetArguments::new(&queue);
            message = channel.basic_get(args).await.unwrap();
        }
        message.unwrap()
    })
    .await
    .unwrap();

    let time_now = OffsetDateTime::now_utc();

    let notification = NotificationProtobuf::decode(bytes.as_slice()).unwrap();
    assert_eq!(notification.user_ids.len(), 1);
    let notification_user_id_str = notification.user_ids.first().unwrap();
    let notification_user_id = Uuid::from_str(notification_user_id_str).unwrap();
    assert_eq!(notification_user_id, user_id);
    assert_eq!(notification.id, id);
    assert_eq!(notification.status(), NotificationStatusProtobuf::Deleted);
    let notification_timestamp = notification.timestamp.unwrap();
    let notification_datetime = OffsetDateTime::from_unix_timestamp(notification_timestamp.seconds)
        .unwrap()
        .replace_nanosecond(notification_timestamp.nanos as u32)
        .unwrap();
    assert!(time_before_send <= notification_datetime && notification_datetime <= time_now);
    assert!(notification.seen.is_none());
    assert!(notification.content_type.is_none());
    assert!(notification.content.is_none());

    destroy_rabbitmq(connection, channel, &queue).await;
}

async fn init_rabbitmq(queue: &str, routing_key: &str) -> (Connection, Channel) {
    let connection_string = std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_CONNECTION_STRING").unwrap();

    let args = OpenConnectionArguments::try_from(connection_string.as_str()).unwrap();
    let connection = Connection::open(&args).await.unwrap();

    let channel = connection.open_channel(None).await.unwrap();

    let exchange = std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME").unwrap();
    let args = QueueDeclareArguments::new(queue);
    channel.queue_declare(args).await.unwrap();
    let args = QueueBindArguments::new(queue, &exchange, routing_key);
    channel.queue_bind(args).await.unwrap();

    (connection, channel)
}

async fn destroy_rabbitmq(connection: Connection, channel: Channel, queue: &str) {
    let args = QueueDeleteArguments::new(queue);
    channel.queue_delete(args).await.unwrap();

    channel.close().await.unwrap();
    connection.close().await.unwrap();
}
