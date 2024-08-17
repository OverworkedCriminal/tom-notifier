mod common;

pub use common::*;
use reqwest::{Client, StatusCode};
use std::sync::Once;
use uuid::Uuid;

static BEFORE_ALL: Once = Once::new();

fn init_env_variables() {
    let _ = dotenvy::dotenv();
}

#[tokio::test]
async fn get_ticket() {
    BEFORE_ALL.call_once(init_env_variables);

    let client = Client::new();

    let response = client
        .get(format!("http://{}/api/v1/ticket", address()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn delete_connection() {
    BEFORE_ALL.call_once(init_env_variables);

    let client = Client::new();

    let response = client
        .delete(format!(
            "http://{}/api/v1/connection/{}",
            address(),
            Uuid::new_v4(),
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_non_existent_uri() {
    BEFORE_ALL.call_once(init_env_variables);

    let client = Client::new();

    let response = client
        .get(format!("http://{}/this-uri-does-not-exist", address()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
