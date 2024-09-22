use amqprs::{
    channel::Channel,
    connection::{Connection, OpenConnectionArguments},
};
use http::StatusCode;
use jwt_auth::test::create_jwt;
use reqwest::Client;
use serde_json::Value;
use std::sync::Once;
use uuid::Uuid;

pub mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/protobuf.rs"));
}

static INIT_ENV_ONCE: Once = Once::new();

pub fn init_env() {
    INIT_ENV_ONCE.call_once(|| {
        let _ = dotenvy::dotenv();
    });
}

pub async fn fetch_ticket(client: &Client, user_id: Uuid) -> anyhow::Result<String> {
    let ticket_response = client
        .get(format!("http://{}/api/v1/ticket", address()))
        .bearer_auth(encode_jwt(user_id, &[]))
        .send()
        .await?;
    assert_eq!(ticket_response.status(), StatusCode::OK);

    let bytes = ticket_response.bytes().await?.slice(..);
    let json_body = serde_json::from_slice::<Value>(&bytes)?;
    let ticket = json_body
        .as_object()
        .unwrap()
        .get("ticket")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    Ok(ticket)
}

pub fn address() -> String {
    std::env::var("TOM_NOTIFIER_WS_DELIVERY_BIND_ADDRESS").unwrap()
}

pub fn ws_url(ticket: &str) -> String {
    format!("ws://{}/ws/v1?ticket={}", address(), ticket)
}

pub async fn init_rabbitmq() -> (Connection, Channel) {
    let connection_string =
        std::env::var("TOM_NOTIFIER_WS_DELIVERY_RABBITMQ_CONNECTION_STRING").unwrap();

    let args = OpenConnectionArguments::try_from(connection_string.as_ref()).unwrap();
    let connection = Connection::open(&args).await.unwrap();

    let channel = connection.open_channel(None).await.unwrap();

    (connection, channel)
}

pub async fn destroy_rabbitmq(connection: Connection, channel: Channel) {
    channel.close().await.unwrap();
    connection.close().await.unwrap();
}

pub fn encode_jwt(user_id: Uuid, roles: &[&str]) -> String {
    let jwt_algorithms = std::env::var("TOM_NOTIFIER_WS_DELIVERY_JWT_ALGORITHMS").unwrap();
    let jwt_key = std::env::var("TOM_NOTIFIER_WS_DELIVERY_JWT_KEY").unwrap();

    create_jwt(user_id, roles, jwt_algorithms, jwt_key)
}
