mod common;

pub use common::*;
use reqwest::{Client, StatusCode};
use uuid::Uuid;

#[tokio::test]
async fn get_ticket() {
    init_env();

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
    init_env();

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
    init_env();

    let client = Client::new();

    let response = client
        .get(format!("http://{}/this-uri-does-not-exist", address()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
