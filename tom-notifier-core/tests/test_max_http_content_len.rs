mod common;
pub use common::*;

use reqwest::{header::CONTENT_TYPE, Client, StatusCode};

#[tokio::test]
async fn max_content_length_http() {
    // sending to large content should result in 413

    let max_content_len: usize = std::env::var("TOM_NOTIFIER_CORE_MAX_HTTP_CONTENT_LEN")
        .unwrap()
        .parse()
        .unwrap();

    // content does not matter, it should be rejected bacause of its size
    let mut content = String::with_capacity(max_content_len + 1);
    for _ in 0..max_content_len + 1 {
        content.push('0');
    }

    let client = Client::new();

    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(create_producer_jwt())
        .header(CONTENT_TYPE, "application/json")
        .body(content)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
