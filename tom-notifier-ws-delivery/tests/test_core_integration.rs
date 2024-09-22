//!
//! Tests for integration with tom-notifier-core
//!
pub mod common;

use common::*;
use futures::{SinkExt, StreamExt};
use http::{header::CONTENT_TYPE, StatusCode};
use prost::Message as ProstMessage;
use protobuf::{
    notification::NotificationStatusProtobuf,
    websocket_confirmation::WebSocketConfirmationProtobuf,
    websocket_notification::WebSocketNotificationProtobuf,
};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use time::OffsetDateTime;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[tokio::test]
async fn created_notification_delivered_to_the_user() -> anyhow::Result<()> {
    init_env();

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let producer_id = Uuid::new_v4();

    let ticket = fetch_ticket(&client, user_id).await?;
    let (mut ws, _) = connect_async(ws_url(&ticket)).await?;

    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            core_address(),
        ))
        .bearer_auth(encode_jwt(
            producer_id,
            &["tom_notifier_produce_notifications"],
        ))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                "user_ids": vec![user_id],
                "producer_notification_id": 1,
                "content_type": "ascii",
                "content": "Y3JlYXRlZF9ub3RpZmljYXRpb25fZGVsaXZlcmVkX3RvX3RoZV91c2Vy",
            })
            .to_string(),
        )
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    let message_bytes = timeout(Duration::from_secs(5), async {
        loop {
            let message = ws.next().await.unwrap().unwrap();
            match message {
                Message::Binary(bytes) => break bytes,
                Message::Ping(ping_message) => {
                    ws.send(Message::Pong(ping_message)).await.unwrap();
                }
                message => panic!("unexpected message: {message:#?}"),
            }
        }
    })
    .await?;
    let message = WebSocketNotificationProtobuf::decode(message_bytes.as_slice())?;
    let notification = message.notification.unwrap();
    assert_eq!(notification.id, saved_notification_id);
    assert_eq!(notification.status(), NotificationStatusProtobuf::New);
    assert_eq!(notification.created_by(), producer_id.to_string());

    Ok(())
}

#[tokio::test]
async fn confirmed_notification_marked_as_delivered() -> anyhow::Result<()> {
    init_env();

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let producer_id = Uuid::new_v4();

    let ticket = fetch_ticket(&client, user_id).await?;
    let (mut ws, _) = connect_async(ws_url(&ticket)).await?;

    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            core_address(),
        ))
        .bearer_auth(encode_jwt(
            producer_id,
            &["tom_notifier_produce_notifications"],
        ))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                "user_ids": vec![user_id],
                "producer_notification_id": 1,
                "content_type": "ascii",
                "content": "Y29uZmlybWVkX25vdGlmaWNhdGlvbl9tYXJrZWRfYXNfZGVsaXZlcmVk",
            })
            .to_string(),
        )
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    let message_bytes = timeout(Duration::from_secs(5), async {
        loop {
            let message = ws.next().await.unwrap().unwrap();
            match message {
                Message::Binary(bytes) => break bytes,
                Message::Ping(ping_message) => {
                    ws.send(Message::Pong(ping_message)).await.unwrap();
                }
                message => panic!("unexpected message: {message:#?}"),
            }
        }
    })
    .await?;

    let message = WebSocketNotificationProtobuf::decode(message_bytes.as_slice())?;
    let confirmation = WebSocketConfirmationProtobuf {
        message_id: message.message_id,
    }
    .encode_to_vec();

    ws.send(Message::Binary(confirmation)).await?;

    sleep(Duration::from_secs(5)).await;

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            core_address(),
            saved_notification_id,
        ))
        .bearer_auth(encode_jwt(user_id, &[]))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    Ok(())
}

#[tokio::test]
async fn updated_notification_delivered_to_the_user() -> anyhow::Result<()> {
    init_env();

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let producer_id = Uuid::new_v4();

    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            core_address(),
        ))
        .bearer_auth(encode_jwt(
            producer_id,
            &["tom_notifier_produce_notifications"],
        ))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                "user_ids": vec![user_id],
                "producer_notification_id": 1,
                "content_type": "ascii",
                "content": "dXBkYXRlZF9ub3RpZmljYXRpb25fZGVsaXZlcmVkX3RvX3RoZV91c2Vy",
            })
            .to_string(),
        )
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            core_address(),
        ))
        .bearer_auth(encode_jwt(user_id, &[]))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let ticket = fetch_ticket(&client, user_id).await?;
    let (mut ws, _) = connect_async(ws_url(&ticket)).await?;

    let response = client
        .put(format!(
            "http://{}/api/v1/notifications/delivered/{}/seen",
            core_address(),
            saved_notification_id,
        ))
        .bearer_auth(encode_jwt(user_id, &[]))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "seen": true,
            })
            .to_string(),
        )
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let message_bytes = timeout(Duration::from_secs(5), async {
        loop {
            let message = ws.next().await.unwrap().unwrap();
            match message {
                Message::Binary(bytes) => break bytes,
                Message::Ping(ping_message) => {
                    ws.send(Message::Pong(ping_message)).await.unwrap();
                }
                message => panic!("unexpected message: {message:#?}"),
            }
        }
    })
    .await?;

    let message = WebSocketNotificationProtobuf::decode(message_bytes.as_slice())?;
    let notification = message.notification.unwrap();
    assert_eq!(notification.id, saved_notification_id);
    assert_eq!(notification.status(), NotificationStatusProtobuf::Updated);
    assert_eq!(notification.seen(), true);

    Ok(())
}

#[tokio::test]
async fn deleted_notification_delivered_to_the_user() -> anyhow::Result<()> {
    init_env();

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let producer_id = Uuid::new_v4();

    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            core_address(),
        ))
        .bearer_auth(encode_jwt(
            producer_id,
            &["tom_notifier_produce_notifications"],
        ))
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(300),
                "user_ids": vec![user_id],
                "producer_notification_id": 1,
                "content_type": "ascii",
                "content": "ZGVsZXRlZF9ub3RpZmljYXRpb25fZGVsaXZlcmVkX3RvX3RoZV91c2Vy",
            })
            .to_string(),
        )
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            core_address(),
        ))
        .bearer_auth(encode_jwt(user_id, &[]))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);

    let ticket = fetch_ticket(&client, user_id).await?;
    let (mut ws, _) = connect_async(ws_url(&ticket)).await?;

    let response = client
        .delete(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            core_address(),
            saved_notification_id,
        ))
        .bearer_auth(encode_jwt(user_id, &[]))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let message_bytes = timeout(Duration::from_secs(5), async {
        loop {
            let message = ws.next().await.unwrap().unwrap();
            match message {
                Message::Binary(bytes) => break bytes,
                Message::Ping(ping_message) => {
                    ws.send(Message::Pong(ping_message)).await.unwrap();
                }
                message => panic!("unexpected message: {message:#?}"),
            }
        }
    })
    .await?;

    let message = WebSocketNotificationProtobuf::decode(message_bytes.as_slice())?;
    let notification = message.notification.unwrap();
    assert_eq!(notification.id, saved_notification_id);
    assert_eq!(notification.status(), NotificationStatusProtobuf::Deleted);

    Ok(())
}

fn core_address() -> String {
    std::env::var("TOM_NOTIFIER_CORE_ADDRESS").unwrap()
}
