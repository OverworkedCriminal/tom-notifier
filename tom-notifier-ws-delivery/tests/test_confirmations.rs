pub mod common;

use amqprs::{
    channel::{
        BasicGetArguments, BasicPublishArguments, Channel, QueueBindArguments,
        QueueDeclareArguments, QueueDeleteArguments,
    },
    connection::{Connection, OpenConnectionArguments},
    BasicProperties, GetOk,
};
use bson::oid::ObjectId;
use common::*;
use futures::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use prost_types::Timestamp;
use reqwest::Client;
use serial_test::serial;
use std::time::Duration;
use time::OffsetDateTime;
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/protobuf.rs"));
}

#[tokio::test]
#[serial]
async fn confirmation_sent_after_receiving_response_to_new() -> anyhow::Result<()> {
    test_confirmation_send_after_response(
        protobuf::notification::NotificationStatusProtobuf::New,
        "test confirmation_sent_after_receiving_response_to_new",
        "NEW",
        |message, id, user_id| {
            let (_get_ok, _basic_properties, content) = message.unwrap();

            let confirmation =
                protobuf::rabbitmq_confirmation::RabbitmqConfirmationProtobuf::decode(
                    content.as_slice(),
                )
                .unwrap();
            assert_eq!(confirmation.id, id.to_hex());
            assert_eq!(confirmation.user_id, user_id.to_string());
        },
    )
    .await
}

#[tokio::test]
#[serial]
async fn confirmation_not_sent_after_receiving_response_to_updated() -> anyhow::Result<()> {
    test_confirmation_send_after_response(
        protobuf::notification::NotificationStatusProtobuf::Updated,
        "test confirmation_not_sent_after_receiving_response_to_updated",
        "UPDATED",
        |message, _, _| assert!(message.is_none()),
    )
    .await
}

#[tokio::test]
#[serial]
async fn confirmation_not_sent_after_receiving_response_to_deleted() -> anyhow::Result<()> {
    test_confirmation_send_after_response(
        protobuf::notification::NotificationStatusProtobuf::Deleted,
        "test confirmation_not_sent_after_receiving_response_to_deleted",
        "DELETED",
        |message, _, _| assert!(message.is_none()),
    )
    .await
}

async fn test_confirmation_send_after_response(
    status: protobuf::notification::NotificationStatusProtobuf,
    queue: &str,
    routing_key: &str,
    assertions_fn: impl FnOnce(Option<(GetOk, BasicProperties, Vec<u8>)>, ObjectId, Uuid),
) -> anyhow::Result<()> {
    init_env();

    let client = Client::new();
    let user_id = Uuid::new_v4();

    let ticket = fetch_ticket(&client, user_id).await?;
    let (mut ws, _) = connect_async(ws_url(&ticket)).await?;

    let (connection, channel) = init_rabbitmq(queue).await;

    let basic_properties = BasicProperties::default();
    let args = BasicPublishArguments::new(
        &std::env::var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_NOTIFICATIONS_EXCHANGE_NAME")?,
        routing_key,
    );
    let id = ObjectId::new();
    let now = OffsetDateTime::now_utc();
    let notification = protobuf::rabbitmq_notification::RabbitmqNotificationProtobuf {
        user_ids: vec![user_id.to_string()],
        notification: Some(protobuf::notification::NotificationProtobuf {
            id: id.to_hex(),
            status: status.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: Some(Uuid::new_v4().to_string()),
            seen: Some(false),
            content_type: Some("utf-8".to_string()),
            content: Some(b"test_confirmation_send_after_response".to_vec()),
        }),
    };
    channel
        .basic_publish(basic_properties, notification.encode_to_vec(), args)
        .await?;

    let websocket_message = timeout(Duration::from_secs(1), ws.next()).await?.unwrap()?;
    let Message::Binary(bytes) = websocket_message else {
        panic!("invalid message type")
    };

    let websocket_message =
        protobuf::websocket_notification::WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
    let message_id = websocket_message.message_id;
    assert_eq!(id.to_hex(), websocket_message.notification.unwrap().id);

    let confirmation =
        protobuf::websocket_confirmation::WebSocketConfirmationProtobuf { message_id };
    let bytes = confirmation.encode_to_vec();

    ws.send(Message::Binary(bytes)).await?;

    // Leave some time for processing
    sleep(Duration::from_millis(500)).await;

    let args = BasicGetArguments::new(queue);
    let queued_message = channel.basic_get(args).await?;

    assertions_fn(queued_message, id, user_id);

    destroy_rabbitmq(connection, channel, queue).await;

    Ok(())
}

async fn init_rabbitmq(queue: &str) -> (Connection, Channel) {
    let connection_string =
        std::env::var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_CONNECTION_STRING").unwrap();

    let args = OpenConnectionArguments::try_from(connection_string.as_ref()).unwrap();
    let connection = Connection::open(&args).await.unwrap();

    let channel = connection.open_channel(None).await.unwrap();

    let exchange =
        std::env::var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_CONFIRMATIONS_EXCHANGE_NAME").unwrap();
    let args = QueueDeclareArguments::new(queue).durable(false).finish();
    channel.queue_declare(args).await.unwrap();
    let args = QueueBindArguments::new(queue, &exchange, "");
    channel.queue_bind(args).await.unwrap();

    (connection, channel)
}

async fn destroy_rabbitmq(connection: Connection, channel: Channel, queue: &str) {
    let args = QueueDeleteArguments::new(queue);
    channel.queue_delete(args).await.unwrap();

    channel.close().await.unwrap();
    connection.close().await.unwrap();
}
