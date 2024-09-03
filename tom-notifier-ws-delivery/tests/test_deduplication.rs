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
use std::{collections::HashSet, future::Future, time::Duration};
use time::OffsetDateTime;
use tokio::{net::TcpStream, time::timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use uuid::Uuid;

#[tokio::test]
async fn deduplication_not_needed() -> anyhow::Result<()> {
    let user_id = Uuid::new_v4();

    let id_1 = ObjectId::new();
    let id_2 = ObjectId::new();

    let datetime_1 = OffsetDateTime::now_utc() - Duration::from_secs(300);
    let datetime_2 = OffsetDateTime::now_utc() - Duration::from_secs(150);

    let notifications = [
        RabbitmqNotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            notification: Some(NotificationProtobuf {
                id: id_1.to_hex(),
                status: NotificationStatusProtobuf::Updated.into(),
                timestamp: Some(Timestamp {
                    seconds: datetime_1.unix_timestamp(),
                    nanos: datetime_1.nanosecond() as i32,
                }),
                created_by: None,
                seen: Some(false),
                content_type: None,
                content: None,
            }),
        },
        RabbitmqNotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            notification: Some(NotificationProtobuf {
                id: id_2.to_hex(),
                status: NotificationStatusProtobuf::Updated.into(),
                timestamp: Some(Timestamp {
                    seconds: datetime_2.unix_timestamp(),
                    nanos: datetime_2.nanosecond() as i32,
                }),
                created_by: None,
                seen: Some(true),
                content_type: None,
                content: None,
            }),
        },
    ];

    let assertions_fn = |mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>| async move {
        // Set of expected IDs that should be received throgh websocket.
        // At the end of test this set should be empty
        let mut expected_ids = HashSet::new();
        expected_ids.insert(id_1.to_hex());
        expected_ids.insert(id_2.to_hex());

        for _ in 0..expected_ids.len() {
            let ws_message = timeout(Duration::from_secs(5), ws.next()).await?.unwrap()?;
            let Message::Binary(bytes) = ws_message else {
                panic!("invalid message type");
            };

            let notification = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
            expected_ids.remove(&notification.notification.unwrap().id);
        }

        assert!(expected_ids.is_empty());

        Ok(())
    };

    test_deduplication(user_id, "UPDATED", &notifications, assertions_fn).await?;

    Ok(())
}

#[tokio::test]
async fn deduplication_same_notification_sent_twice() -> anyhow::Result<()> {
    let user_id = Uuid::new_v4();

    let id = ObjectId::new();
    let datetime = OffsetDateTime::now_utc() - Duration::from_secs(300);

    let notification = RabbitmqNotificationProtobuf {
        user_ids: vec![user_id.to_string()],
        notification: Some(NotificationProtobuf {
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: datetime.unix_timestamp(),
                nanos: datetime.nanosecond() as i32,
            }),
            created_by: None,
            seen: Some(false),
            content_type: None,
            content: None,
        }),
    };
    let notifications = [notification.clone(), notification.clone()];

    let assertions_fn = |mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>| async move {
        // only one message should be received
        let timeout_1 = timeout(Duration::from_secs(5), ws.next()).await;
        let timeout_2 = timeout(Duration::from_secs(5), ws.next()).await;
        assert!(timeout_2.is_err());

        let ws_message = timeout_1?.unwrap()?;
        let Message::Binary(bytes) = ws_message else {
            panic!("invalid message type");
        };
        let notification = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
        assert_eq!(notification.notification.unwrap().id, id.to_hex());

        Ok(())
    };

    test_deduplication(user_id, "UPDATED", &notifications, assertions_fn).await?;

    Ok(())
}

#[tokio::test]
async fn deduplication_older_notifications_are_ignored() -> anyhow::Result<()> {
    let user_id = Uuid::new_v4();

    let id = ObjectId::new();

    let datetime_1 = OffsetDateTime::now_utc();
    let datetime_2 = OffsetDateTime::now_utc() - Duration::from_secs(300);

    let notifications = [
        RabbitmqNotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            notification: Some(NotificationProtobuf {
                id: id.to_hex(),
                status: NotificationStatusProtobuf::Updated.into(),
                timestamp: Some(Timestamp {
                    seconds: datetime_1.unix_timestamp(),
                    nanos: datetime_1.nanosecond() as i32,
                }),
                created_by: None,
                seen: Some(false),
                content_type: None,
                content: None,
            }),
        },
        RabbitmqNotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            notification: Some(NotificationProtobuf {
                id: id.to_hex(),
                status: NotificationStatusProtobuf::Updated.into(),
                timestamp: Some(Timestamp {
                    seconds: datetime_2.unix_timestamp(),
                    nanos: datetime_2.nanosecond() as i32,
                }),
                created_by: None,
                seen: Some(true),
                content_type: None,
                content: None,
            }),
        },
    ];

    let assertions_fn = |mut ws: WebSocketStream<MaybeTlsStream<TcpStream>>| async move {
        // only first message should be received
        let timeout_1 = timeout(Duration::from_secs(5), ws.next()).await;
        let timeout_2 = timeout(Duration::from_secs(5), ws.next()).await;
        assert!(timeout_2.is_err());

        let ws_message = timeout_1?.unwrap()?;
        let Message::Binary(bytes) = ws_message else {
            panic!("invalid message type");
        };
        let notification = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
        assert_eq!(notification.notification.unwrap().id, id.to_hex());

        Ok(())
    };

    test_deduplication(user_id, "UPDATED", &notifications, assertions_fn).await?;

    Ok(())
}

async fn test_deduplication<F, Fut>(
    user_id: Uuid,
    routing_key: &str,
    notifications: &[RabbitmqNotificationProtobuf],
    assertions_fn: F,
) -> anyhow::Result<()>
where
    F: FnOnce(WebSocketStream<MaybeTlsStream<TcpStream>>) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    init_env();

    let client = Client::new();

    let ticket = fetch_ticket(&client, user_id).await?;
    let (ws, _) = connect_async(ws_url(&ticket)).await?;

    let (connection, channel) = init_rabbitmq().await;

    let exchange = std::env::var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME")?;

    // Send all notifications
    for notification in notifications {
        let basic_properties = BasicProperties::default();
        let bytes = notification.encode_to_vec();
        let args = BasicPublishArguments::new(&exchange, routing_key);
        channel.basic_publish(basic_properties, bytes, args).await?;
    }

    assertions_fn(ws).await?;

    destroy_rabbitmq(connection, channel).await;

    Ok(())
}
