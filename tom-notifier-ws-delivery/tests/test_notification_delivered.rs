pub mod common;

use amqprs::{channel::BasicPublishArguments, BasicProperties};
use bson::oid::ObjectId;
use common::*;
use futures::StreamExt;
use prost::Message as ProstMessage;
use prost_types::Timestamp;
use protobuf::{
    notification::{NotificationProtobuf, NotificationStatusProtobuf},
    rabbitmq_notification::RabbitmqNotificationProtobuf,
    websocket_notification::WebSocketNotificationProtobuf,
};
use reqwest::Client;
use serial_test::{parallel, serial};
use std::{future::Future, time::Duration};
use time::OffsetDateTime;
use tokio::{
    net::TcpStream,
    time::{sleep, timeout},
};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

// Unicast

#[tokio::test]
#[parallel]
async fn notification_delivered_to_the_user() -> anyhow::Result<()> {
    let id = ObjectId::new();
    let user_ids = [Uuid::new_v4()];

    let now = OffsetDateTime::now_utc();
    let notification = RabbitmqNotificationProtobuf {
        user_ids: user_ids.into_iter().map(|uuid| uuid.to_string()).collect(),
        notification: Some(NotificationProtobuf {
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        }),
    };

    let assertions_fn = |mut websockets: Vec<WebSocketStream<MaybeTlsStream<TcpStream>>>| async move {
        let ws = websockets.first_mut().unwrap();

        let ws_message = timeout(Duration::from_secs(5), ws.next()).await?.unwrap()?;
        let Message::Binary(bytes) = ws_message else {
            panic!("invalid message type");
        };
        let ws_message = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
        assert_eq!(ws_message.notification.unwrap().id, id.to_hex());

        Ok(())
    };

    test_notification_delivered(&user_ids, notification, assertions_fn).await?;

    Ok(())
}

#[tokio::test]
#[parallel]
async fn notification_delivered_to_all_user_devices() -> anyhow::Result<()> {
    let id = ObjectId::new();
    let user_id = Uuid::new_v4();
    // Connect multiple devices of the same user
    let user_ids = [user_id, user_id, user_id];

    let now = OffsetDateTime::now_utc();
    let notification = RabbitmqNotificationProtobuf {
        user_ids: user_ids.into_iter().map(|uuid| uuid.to_string()).collect(),
        notification: Some(NotificationProtobuf {
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        }),
    };

    let assertions_fn = |websockets: Vec<WebSocketStream<MaybeTlsStream<TcpStream>>>| async move {
        let mut tasks = Vec::with_capacity(websockets.len());
        for mut ws in websockets {
            let task =
                tokio::spawn(async move { timeout(Duration::from_secs(5), ws.next()).await });
            tasks.push(task);
        }

        let mut timeouts = Vec::with_capacity(tasks.len());
        for task in tasks {
            let timeout = task.await?;
            timeouts.push(timeout);
        }

        for timeout in timeouts {
            let ws_message = timeout?.unwrap()?;
            let Message::Binary(bytes) = ws_message else {
                panic!("invalid message type");
            };
            let ws_message = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
            assert_eq!(ws_message.notification.unwrap().id, id.to_hex());
        }

        Ok(())
    };

    test_notification_delivered(&user_ids, notification, assertions_fn).await?;

    Ok(())
}

#[tokio::test]
#[parallel]
async fn notification_delivered_after_other_device_disconnected() -> anyhow::Result<()> {
    init_env();

    let id = ObjectId::new();
    let user_id = Uuid::new_v4();

    let now = OffsetDateTime::now_utc();
    let notification = RabbitmqNotificationProtobuf {
        user_ids: vec![user_id.to_string()],
        notification: Some(NotificationProtobuf {
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        }),
    };

    let client = Client::new();
    let mut websockets = Vec::with_capacity(2);
    for user_id in [user_id, user_id] {
        let ticket = fetch_ticket(&client, user_id).await?;
        let (ws, _) = connect_async(ws_url(&ticket)).await?;
        websockets.push(ws);
    }

    let (connection, channel) = init_rabbitmq().await;

    let exchange = std::env::var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME")?;

    // Send first notification
    let basic_properties = BasicProperties::default();
    let bytes = notification.encode_to_vec();
    let args = BasicPublishArguments::new(&exchange, "UPDATED");
    channel.basic_publish(basic_properties, bytes, args).await?;

    // All devices should receive a message
    for ws in websockets.iter_mut() {
        let ws_message = timeout(Duration::from_secs(5), ws.next()).await?.unwrap()?;
        let Message::Binary(bytes) = ws_message else {
            panic!("invalid message type");
        };
        let ws_message = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
        assert_eq!(ws_message.notification.unwrap().id, id.to_hex());
    }

    // Close one websocket connection
    let mut ws = websockets.pop().unwrap();
    ws.close(None).await?;

    // Leave some time for server processing
    sleep(Duration::from_millis(500)).await;

    let id = ObjectId::new();
    let now = OffsetDateTime::now_utc();
    let notification = RabbitmqNotificationProtobuf {
        user_ids: vec![user_id.to_string()],
        notification: Some(NotificationProtobuf {
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        }),
    };

    // Send second notification
    let basic_properties = BasicProperties::default();
    let bytes = notification.encode_to_vec();
    let args = BasicPublishArguments::new(&exchange, "UPDATED");
    channel.basic_publish(basic_properties, bytes, args).await?;

    // Remaining notification should receive a message
    let mut ws = websockets.pop().unwrap();
    let ws_message = timeout(Duration::from_secs(5), ws.next()).await?.unwrap()?;
    let Message::Binary(bytes) = ws_message else {
        panic!("invalid message type");
    };
    let ws_message = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
    assert_eq!(ws_message.notification.unwrap().id, id.to_hex());

    destroy_rabbitmq(connection, channel).await;

    Ok(())
}

// Multicast

#[tokio::test]
#[parallel]
async fn notification_delivered_only_to_selected_users() -> anyhow::Result<()> {
    let id = ObjectId::new();
    let user_1_id = Uuid::new_v4();
    let user_2_id = Uuid::new_v4();
    let user_3_id = Uuid::new_v4();
    // Connect multiple users
    let user_ids = [user_1_id, user_2_id, user_3_id];

    let now = OffsetDateTime::now_utc();
    let notification = RabbitmqNotificationProtobuf {
        // user_2 should not receive message
        user_ids: vec![user_1_id.to_string(), user_3_id.to_string()],
        notification: Some(NotificationProtobuf {
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        }),
    };

    let assertions_fn = |websockets: Vec<WebSocketStream<MaybeTlsStream<TcpStream>>>| async move {
        let mut tasks = Vec::with_capacity(websockets.len());
        for mut ws in websockets {
            let task =
                tokio::spawn(async move { timeout(Duration::from_secs(5), ws.next()).await });
            tasks.push(task);
        }

        let mut timeouts = Vec::with_capacity(tasks.len());
        for task in tasks {
            let timeout = task.await?;
            timeouts.push(timeout);
        }

        // user_2 is removed from list because he should not receive message
        let user_2_timeout = timeouts.remove(1);
        assert!(user_2_timeout.is_err());

        // user_1, user_3 should receive message
        for timeout in timeouts {
            let ws_message = timeout?.unwrap()?;
            let Message::Binary(bytes) = ws_message else {
                panic!("invalid message type");
            };
            let ws_message = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
            assert_eq!(ws_message.notification.unwrap().id, id.to_hex());
        }

        Ok(())
    };

    test_notification_delivered(&user_ids, notification, assertions_fn).await?;

    Ok(())
}

// Broadcast

#[tokio::test]
#[serial]
async fn notification_delivered_to_all_users() -> anyhow::Result<()> {
    let id = ObjectId::new();
    let user_1_id = Uuid::new_v4();
    let user_2_id = Uuid::new_v4();
    let user_3_id = Uuid::new_v4();
    // Connect multiple users
    let user_ids = [user_1_id, user_2_id, user_3_id];

    let now = OffsetDateTime::now_utc();
    let notification = RabbitmqNotificationProtobuf {
        user_ids: vec![],
        notification: Some(NotificationProtobuf {
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: None,
            seen: Some(true),
            content_type: None,
            content: None,
        }),
    };

    let assertions_fn = |websockets: Vec<WebSocketStream<MaybeTlsStream<TcpStream>>>| async move {
        let mut tasks = Vec::with_capacity(websockets.len());
        for mut ws in websockets {
            let task =
                tokio::spawn(async move { timeout(Duration::from_secs(5), ws.next()).await });
            tasks.push(task);
        }

        let mut timeouts = Vec::with_capacity(tasks.len());
        for task in tasks {
            let timeout = task.await?;
            timeouts.push(timeout);
        }

        for timeout in timeouts {
            let ws_message = timeout?.unwrap()?;
            let Message::Binary(bytes) = ws_message else {
                panic!("invalid message type");
            };
            let ws_message = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
            assert_eq!(ws_message.notification.unwrap().id, id.to_hex());
        }

        Ok(())
    };

    test_notification_delivered(&user_ids, notification, assertions_fn).await?;

    Ok(())
}

async fn test_notification_delivered<F, Fut>(
    user_ids_to_connect: &[Uuid],
    notification: RabbitmqNotificationProtobuf,
    assertions_fn: F,
) -> anyhow::Result<()>
where
    F: FnOnce(Vec<WebSocketStream<MaybeTlsStream<TcpStream>>>) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    init_env();

    let client = Client::new();
    let mut websockets = Vec::with_capacity(user_ids_to_connect.len());
    for user_id in user_ids_to_connect {
        let ticket = fetch_ticket(&client, *user_id).await?;
        let (ws, _) = connect_async(ws_url(&ticket)).await?;
        websockets.push(ws);
    }

    let (connection, channel) = init_rabbitmq().await;

    let exchange = std::env::var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME")?;

    let basic_properties = BasicProperties::default();
    let bytes = notification.encode_to_vec();
    let args = BasicPublishArguments::new(&exchange, "UPDATED");
    channel.basic_publish(basic_properties, bytes, args).await?;

    assertions_fn(websockets).await?;

    destroy_rabbitmq(connection, channel).await;

    Ok(())
}
