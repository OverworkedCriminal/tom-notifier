//!
//! Module testing if all paths are protected by auth middleware.
//!
//! Any request should return 401 if URI and method is correct, 404 otherwise
//!
mod common;
use common::*;

use bson::oid::ObjectId;
use reqwest::{Client, StatusCode};

#[tokio::test]
async fn post_notifications_undelivered() {
    let client = Client::new();

    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_notifications_undelivered() {
    let client = Client::new();

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn put_notifications_undelivered_invalidate_at() {
    let client = Client::new();

    let response = client
        .put(format!(
            "http://{}/api/v1/notifications/undelivered/{}/invalidate_at",
            address(),
            ObjectId::new().to_hex()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_notifications_delivered() {
    let client = Client::new();

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered",
            address()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_notification_delivered() {
    let client = Client::new();

    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            ObjectId::new().to_hex()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn delete_notification_delivered() {
    let client = Client::new();

    let response = client
        .delete(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            ObjectId::new().to_hex()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn put_notification_delivered_seen() {
    let client = Client::new();

    let response = client
        .put(format!(
            "http://{}/api/v1/notifications/delivered/{}/seen",
            address(),
            ObjectId::new().to_hex()
        ))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn get_non_existent_uri() {
    let client = Client::new();

    let response = client
        .get(format!("http://{}/this-uri-does-not-exist", address(),))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
