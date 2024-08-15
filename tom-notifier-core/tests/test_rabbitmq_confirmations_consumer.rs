mod common;

use amqprs::{
    channel::BasicPublishArguments,
    connection::{Connection, OpenConnectionArguments},
    BasicProperties,
};
pub use common::*;
use prost::Message;
use protobuf::confirmation::ConfirmationProtobuf;
use reqwest::{header::CONTENT_TYPE, Client, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;
use time::OffsetDateTime;
use tokio::time::sleep;
use uuid::Uuid;

mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/protobuf.rs"));
}

#[tokio::test]
async fn confirmed_message_does_not_appear_in_undelivered_notifications() {
    let client = Client::new();
    let user_id = Uuid::new_v4();
    let user = create_consumer_jwt_with_id(user_id);
    let producer = create_producer_jwt();

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
                "content_type": "utf-8",
                "content": "Y29uZmlybWVkX21lc3NhZ2VfZG9lc19ub3RfYXBwZWFyX2luX3VuZGVsaXZlcmVkX25vdGlmaWNhdGlvbnM=",
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

    let connection_string = std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_CONNECTION_STRING").unwrap();
    let args = OpenConnectionArguments::try_from(connection_string.as_str()).unwrap();
    let connection = Connection::open(&args).await.unwrap();
    let channel = connection.open_channel(None).await.unwrap();

    let exchange = std::env::var("TOM_NOTIFIER_CORE_RABBITMQ_CONFIRMATIONS_EXCHANGE_NAME").unwrap();

    let basic_properties = BasicProperties::default();
    let confirmation = ConfirmationProtobuf {
        id: id.to_string(),
        user_id: user_id.to_string(),
    };
    let content = confirmation.encode_to_vec();
    let args = BasicPublishArguments::new(&exchange, "");
    channel
        .basic_publish(basic_properties, content, args)
        .await
        .unwrap();

    // sleep so server has some time for processing
    sleep(Duration::from_secs(1)).await;

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            id,
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    // if message was not confirmed it will be NOT_FOUND
    assert_eq!(response.status(), StatusCode::OK);

    channel.close().await.unwrap();
    connection.close().await.unwrap();
}
