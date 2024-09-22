pub mod common;

use common::*;
use futures::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use protobuf::{
    websocket_confirmation::WebSocketConfirmationProtobuf,
    websocket_notification::{NetworkStatusProtobuf, WebSocketNotificationProtobuf},
};
use reqwest::Client;
use serial_test::serial;
use std::{process::Command, time::Duration};
use tokio::time::{sleep, timeout};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use uuid::Uuid;

#[tokio::test]
#[serial]
async fn network_status_update_received_on_rabbitmq_failure_and_recovery() -> anyhow::Result<()> {
    init_env();

    let user_id = Uuid::new_v4();
    let client = Client::new();

    let ticket = fetch_ticket(&client, user_id).await?;
    let (mut ws, _) = connect_async(ws_url(&ticket)).await?;

    // restart docker container to simulate network failure
    Command::new("docker")
        .arg("compose")
        .arg("restart")
        .arg("rabbitmq")
        .output()
        .unwrap();

    // Await network status error
    let ws_message = timeout(Duration::from_secs(5), ws.next()).await?.unwrap()?;
    let Message::Binary(bytes) = ws_message else {
        panic!("invalid message type");
    };
    let ws_network_error = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
    assert_eq!(
        ws_network_error.network_status(),
        NetworkStatusProtobuf::Error
    );
    assert!(ws_network_error.notification.is_none());

    // Respond with confirmation
    let confirmation = WebSocketConfirmationProtobuf {
        message_id: ws_network_error.message_id.clone(),
    };
    ws.send(Message::Binary(confirmation.encode_to_vec()))
        .await?;

    // Await network status ok
    timeout(Duration::from_secs(120), async move {
        loop {
            let ws_message = ws.next().await.unwrap().unwrap();
            match ws_message {
                Message::Binary(bytes) => {
                    let ws_message =
                        WebSocketNotificationProtobuf::decode(bytes.as_slice()).unwrap();
                    if ws_message.message_id == ws_network_error.message_id {
                        // It's not a problem if the same message was resent.
                        // No confirmation is sent here because was sent earlier.
                        continue;
                    }
                    assert_eq!(ws_message.network_status(), NetworkStatusProtobuf::Ok);
                    assert!(ws_message.notification.is_none());
                    break;
                }
                Message::Ping(message) => ws.send(Message::Pong(message)).await.unwrap(),
                _ => panic!("invalid message type"),
            }
        }
    })
    .await?;

    Ok(())
}

#[tokio::test]
#[serial]
async fn network_status_error_received_when_rabbitmq_is_down_on_connection() -> anyhow::Result<()> {
    init_env();

    let user_id = Uuid::new_v4();
    let client = Client::new();

    // Restart docker container to simulate network failure
    Command::new("docker")
        .arg("compose")
        .arg("restart")
        .arg("rabbitmq")
        .output()
        .unwrap();

    // Server needs some time to register broken rabbitmq connection
    sleep(Duration::from_millis(100)).await;

    let ticket = fetch_ticket(&client, user_id).await?;
    let (mut ws, _) = connect_async(ws_url(&ticket)).await?;

    // Await network status error
    let ws_message = timeout(Duration::from_secs(5), ws.next()).await?.unwrap()?;
    let Message::Binary(bytes) = ws_message else {
        panic!("invalid message type");
    };
    let ws_message = WebSocketNotificationProtobuf::decode(bytes.as_slice())?;
    assert_eq!(ws_message.network_status(), NetworkStatusProtobuf::Error);
    assert!(ws_message.notification.is_none());

    Ok(())
}
