mod common;
pub use common::*;

use reqwest::{header::CONTENT_TYPE, Client, StatusCode};
use serde_json::{json, Value};
use serial_test::{parallel, serial};
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

// UNICAST

#[tokio::test]
#[parallel]
async fn get_undelivered_notifications_after_producing_them() {
    init_env();

    // after producing notification
    // fetching undelivered notifications should return produced notification
    // fetching undelivered notifications again should return an empty array

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let user = create_consumer_jwt_with_id(user_id);
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": None as Option<OffsetDateTime>,
                "user_ids": [user_id],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "RG9uJ3QgeW91IGRhcmUgZGVjb2RpbmcgbWUh"
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    // fetch undelivered notifications
    // it should contain new notification
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let found_notifications = serde_json::from_slice::<Vec<Value>>(&response_body).unwrap();
    let found_saved_notification = found_notifications
        .iter()
        .map(|value| value.get("id").unwrap().as_str().unwrap())
        .any(|id| id == saved_notification_id);
    assert!(found_saved_notification);

    // fetch undelivered notifications
    // it should return an empty list
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let found_notifications = serde_json::from_slice::<Vec<Value>>(&response_body).unwrap();
    assert!(found_notifications.is_empty());
}

#[tokio::test]
#[parallel]
async fn get_delivered_not_find_anything_unless_find_undelivered_was_called() {
    init_env();

    // after producing notification
    // fetching delivered notification should return 404, because notifications
    // stay in "undelivered" state until get_undelivered is called

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let user = create_consumer_jwt_with_id(user_id);
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": None as Option<OffsetDateTime>,
                "user_ids": [user_id],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "RG9uJ3QgeW91IGRhcmUgZGVjb2RpbmcgbWUh"
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    // fetch delivered by ID
    // should return 404, because notification is in 'undelivered' status
    // until get_undelivered is called
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id,
        ))
        .bearer_auth(user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[parallel]
async fn get_delivered_find_all_notifications() {
    init_env();

    // after producing and fetching undelivered notifications
    // get_delivered should return the same records that were fetched before

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let user = create_consumer_jwt_with_id(user_id);
    let producer = create_producer_jwt();

    let notifications = [
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_id],
            "producer_notification_id": 1,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDEgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_id],
            "producer_notification_id": 2,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDIgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_id],
            "producer_notification_id": 3,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDMgY29udGVudA=="
        })
        .to_string(),
    ];

    // create notifications
    let mut notifications_ids: [String; 3] = Default::default();
    for (i, notification) in notifications.into_iter().enumerate() {
        let response = client
            .post(format!(
                "http://{}/api/v1/notifications/undelivered",
                address()
            ))
            .bearer_auth(&producer)
            .header(CONTENT_TYPE, "application/json")
            .body(notification)
            .send()
            .await
            .unwrap();
        let response_body = response.bytes().await.unwrap();
        let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
        let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

        notifications_ids[i] = saved_notification_id.to_string();
    }

    // fetch undelivered to change their state to delivered
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let found_undelivered_notifications_ids = serde_json::from_slice::<Vec<Value>>(&response_body)
        .unwrap()
        .into_iter()
        .map(|value| value.get("id").unwrap().as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let found_all_ids = notifications_ids
        .iter()
        .all(|id| found_undelivered_notifications_ids.contains(id));
    assert!(found_all_ids);

    // fetch delivered and check whether they contain all saved ids
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered?page_idx=0&page_size=10",
            address()
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let found_delivered_notifications_ids = serde_json::from_slice::<Vec<Value>>(&response_body)
        .unwrap()
        .into_iter()
        .map(|value| value.get("id").unwrap().as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let found_all_ids = notifications_ids
        .iter()
        .all(|id| found_delivered_notifications_ids.contains(id));
    assert!(found_all_ids);
}

#[tokio::test]
#[parallel]
async fn get_delivered_find_all_notifications_one_by_one() {
    init_env();

    // after producing and fetching undelivered notifications
    // they should be marked as delivered so fetching them by ID should be possible

    let client = Client::new();
    let user_id = Uuid::new_v4();
    let user = create_consumer_jwt_with_id(user_id);
    let producer = create_producer_jwt();

    let notifications = [
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_id],
            "producer_notification_id": 1,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDEgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_id],
            "producer_notification_id": 2,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDIgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_id],
            "producer_notification_id": 3,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDMgY29udGVudA=="
        })
        .to_string(),
    ];

    // create notifications
    let mut notifications_ids: [String; 3] = Default::default();
    for (i, notification) in notifications.into_iter().enumerate() {
        let response = client
            .post(format!(
                "http://{}/api/v1/notifications/undelivered",
                address()
            ))
            .bearer_auth(&producer)
            .header(CONTENT_TYPE, "application/json")
            .body(notification)
            .send()
            .await
            .unwrap();
        let response_body = response.bytes().await.unwrap();
        let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
        let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

        notifications_ids[i] = saved_notification_id.to_string();
    }

    // fetch undelivered to change their state to delivered
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&user)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let found_undelivered_notifications_ids = serde_json::from_slice::<Vec<Value>>(&response_body)
        .unwrap()
        .into_iter()
        .map(|value| value.get("id").unwrap().as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    let found_all_ids = notifications_ids
        .iter()
        .all(|id| found_undelivered_notifications_ids.contains(id));
    assert!(found_all_ids);

    // check whether fetching notifications by their ids is possible
    for id in notifications_ids {
        let response = client
            .get(format!(
                "http://{}/api/v1/notifications/delivered/{}",
                address(),
                id,
            ))
            .bearer_auth(&user)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

// MULTICAST

#[tokio::test]
#[parallel]
async fn multicast_get_undelivered_notifications_received_by_all_users() {
    init_env();

    // after producing notifications
    // all recipients should be able to fetch them

    let client = Client::new();
    let user_1_id = Uuid::new_v4();
    let user_2_id = Uuid::new_v4();
    let user_1 = create_consumer_jwt_with_id(user_1_id);
    let user_2 = create_consumer_jwt_with_id(user_2_id);
    let producer = create_producer_jwt();

    let notifications = [
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_1_id, user_2_id],
            "producer_notification_id": 1,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDEgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_1_id, user_2_id],
            "producer_notification_id": 2,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDIgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_1_id, user_2_id],
            "producer_notification_id": 3,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDMgY29udGVudA=="
        })
        .to_string(),
    ];

    // create notifications
    let mut notifications_ids: [String; 3] = Default::default();
    for (i, notification) in notifications.into_iter().enumerate() {
        let response = client
            .post(format!(
                "http://{}/api/v1/notifications/undelivered",
                address()
            ))
            .bearer_auth(&producer)
            .header(CONTENT_TYPE, "application/json")
            .body(notification)
            .send()
            .await
            .unwrap();
        let response_body = response.bytes().await.unwrap();
        let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
        let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

        notifications_ids[i] = saved_notification_id.to_string();
    }

    // fetch undelivered for each user and check
    // whether they contain all saved notifications
    for user in [user_1, user_2] {
        let response = client
            .get(format!(
                "http://{}/api/v1/notifications/undelivered",
                address()
            ))
            .bearer_auth(&user)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response_body = response.bytes().await.unwrap();
        let found_undelivered_notifications_ids =
            serde_json::from_slice::<Vec<Value>>(&response_body)
                .unwrap()
                .into_iter()
                .map(|value| value.get("id").unwrap().as_str().unwrap().to_string())
                .collect::<Vec<_>>();
        let found_all_ids = notifications_ids
            .iter()
            .all(|id| found_undelivered_notifications_ids.contains(id));
        assert!(found_all_ids);
    }
}

#[tokio::test]
#[parallel]
async fn multicast_get_undelivered_does_not_affect_other_users() {
    init_env();

    // after producing notifications
    // when user_1 fetch undelivered notifications
    // user_2 notifications still should be in undelivered state

    let client = Client::new();
    let user_1_id = Uuid::new_v4();
    let user_2_id = Uuid::new_v4();
    let user_1 = create_consumer_jwt_with_id(user_1_id);
    let user_2 = create_consumer_jwt_with_id(user_2_id);
    let producer = create_producer_jwt();

    let notifications = [
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_1_id, user_2_id],
            "producer_notification_id": 1,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDEgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_1_id, user_2_id],
            "producer_notification_id": 2,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDIgY29udGVudA=="
        })
        .to_string(),
        json!({
            "invalidate_at": None as Option<OffsetDateTime>,
            "user_ids": [user_1_id, user_2_id],
            "producer_notification_id": 3,
            "content_type": "utf-8",
            "content": "bm90aWZpY2F0aW9uIDMgY29udGVudA=="
        })
        .to_string(),
    ];

    // create notifications
    let mut notifications_ids: [String; 3] = Default::default();
    for (i, notification) in notifications.into_iter().enumerate() {
        let response = client
            .post(format!(
                "http://{}/api/v1/notifications/undelivered",
                address()
            ))
            .bearer_auth(&producer)
            .header(CONTENT_TYPE, "application/json")
            .body(notification)
            .send()
            .await
            .unwrap();
        let response_body = response.bytes().await.unwrap();
        let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
        let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

        notifications_ids[i] = saved_notification_id.to_string();
    }

    // fetch undelivered for user_1
    // it should not affect user_2
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&user_1)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // fetch each notification by ID for user_2
    // they all should not exist to him
    for id in notifications_ids {
        let response = client
            .get(format!(
                "http://{}/api/v1/notifications/delivered/{}",
                address(),
                id,
            ))
            .bearer_auth(&user_2)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

#[tokio::test]
#[parallel]
async fn multicast_update_seen_does_not_affect_other_users() {
    init_env();

    // after producing and fetching undelivered notifications
    // when user_1 change status to 'seen' it should not affect user_2

    let client = Client::new();
    let user_1_id = Uuid::new_v4();
    let user_2_id = Uuid::new_v4();
    let user_1 = create_consumer_jwt_with_id(user_1_id);
    let user_2 = create_consumer_jwt_with_id(user_2_id);
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": None as Option<OffsetDateTime>,
                "user_ids": [user_1_id, user_2_id],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "RG9uJ3QgeW91IGRhcmUgZGVjb2RpbmcgbWUh"
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    // fetch undelivered notifications for both users
    for user in [&user_1, &user_2] {
        let response = client
            .get(format!(
                "http://{}/api/v1/notifications/undelivered",
                address()
            ))
            .bearer_auth(&user)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // user_1 updates seen to 'true'
    let response = client
        .put(format!(
            "http://{}/api/v1/notifications/delivered/{}/seen",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&user_1)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "seen": true
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // fetch notification for user_1
    // seen should be updated to 'true'
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&user_1)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let found_notification = serde_json::from_slice::<Value>(&response_body).unwrap();
    let seen = found_notification.get("seen").unwrap().as_bool().unwrap();
    assert_eq!(seen, true);

    // fetch notification for user_2
    // seen should be still 'false'
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&user_2)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let found_notification = serde_json::from_slice::<Value>(&response_body).unwrap();
    let seen = found_notification.get("seen").unwrap().as_bool().unwrap();
    assert_eq!(seen, false);
}

#[tokio::test]
#[parallel]
async fn multicast_delete_does_not_affect_other_users() {
    init_env();

    // after producing and fetching undelivered notifications
    // when user_1 delete notification it should not affect user_2

    let client = Client::new();
    let user_1_id = Uuid::new_v4();
    let user_2_id = Uuid::new_v4();
    let user_1 = create_consumer_jwt_with_id(user_1_id);
    let user_2 = create_consumer_jwt_with_id(user_2_id);
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": None as Option<OffsetDateTime>,
                "user_ids": [user_1_id, user_2_id],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "RG9uJ3QgeW91IGRhcmUgZGVjb2RpbmcgbWUh"
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body.get("id").unwrap().as_str().unwrap();

    // fetch undelivered notifications for both users
    for user in [&user_1, &user_2] {
        let response = client
            .get(format!(
                "http://{}/api/v1/notifications/undelivered",
                address()
            ))
            .bearer_auth(&user)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    // user_1 deletes notification
    let response = client
        .delete(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&user_1)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // fetch notification for user_1
    // it should not exist
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&user_1)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // fetch notification for user_2
    // it should still exist
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&user_2)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

// BROADCAST

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn broadcast_get_undelivered_notifications_received_by_all_users() {
    init_env();

    // after producing notification
    // all users should be able to fetch it

    let client = Client::new();
    let producer = create_producer_jwt();

    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(&producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(30),
                "user_ids": [],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "bm90aWZpY2F0aW9uIDEgY29udGVudA=="
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body
        .get("id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let mut tasks = Vec::with_capacity(10);

    // fetch notifications for each user
    // and check whether they contain only one saved notification
    for _ in 0..10 {
        let client_clone = client.clone();
        let saved_notification_id_clone = saved_notification_id.clone();
        let task = tokio::spawn(async move {
            let response = client_clone
                .get(format!(
                    "http://{}/api/v1/notifications/undelivered",
                    address()
                ))
                .bearer_auth(create_consumer_jwt())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let response_body = response.bytes().await.unwrap();
            let found_notifications = serde_json::from_slice::<Vec<Value>>(&response_body).unwrap();
            let found_saved_notification = found_notifications
                .iter()
                .map(|value| value.get("id").unwrap().as_str().unwrap())
                .any(|id| id == saved_notification_id_clone.as_str());
            assert!(found_saved_notification);
        });
        tasks.push(task);
    }

    // await tasks to propagate panic
    for task in tasks {
        task.await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn broadcast_update_seen_does_not_affect_other_users() {
    init_env();

    // after producing and fetching undelivered notifications
    // when any user changes status to 'seen' it should not affect other users

    let client = Client::new();
    let users = (0..10).map(|_| create_consumer_jwt()).collect::<Vec<_>>();
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(30),
                "user_ids": [],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "RG9uJ3QgeW91IGRhcmUgZGVjb2RpbmcgbWUh"
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body
        .get("id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let mut tasks = Vec::with_capacity(users.len());

    // fetch undelivered notifications for all users
    for user in users.iter() {
        let client_clone = client.clone();
        let user_clone = user.clone();
        let task = tokio::spawn(async move {
            let response = client_clone
                .get(format!(
                    "http://{}/api/v1/notifications/undelivered",
                    address()
                ))
                .bearer_auth(&user_clone)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
        tasks.push(task);
    }

    // await tasks to propagate panics
    while !tasks.is_empty() {
        let task = tasks.pop().unwrap();
        task.await.unwrap();
    }

    // users[0] updates seen to 'true'
    let response = client
        .put(format!(
            "http://{}/api/v1/notifications/delivered/{}/seen",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&users[0])
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "seen": true
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // fetch notification for users[0]
    // seen should be updated to 'true'
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&users[0])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let found_notification = serde_json::from_slice::<Value>(&response_body).unwrap();
    let seen = found_notification.get("seen").unwrap().as_bool().unwrap();
    assert_eq!(seen, true);

    // fetch notification for rest of the users
    // seen should be still 'false'
    for user in &users[1..] {
        let client_clone = client.clone();
        let user_clone = user.clone();
        let saved_notification_id_clone = saved_notification_id.clone();
        let task = tokio::spawn(async move {
            let response = client_clone
                .get(format!(
                    "http://{}/api/v1/notifications/delivered/{}",
                    address(),
                    saved_notification_id_clone
                ))
                .bearer_auth(&user_clone)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let response_body = response.bytes().await.unwrap();
            let found_notification = serde_json::from_slice::<Value>(&response_body).unwrap();
            let seen = found_notification.get("seen").unwrap().as_bool().unwrap();
            assert_eq!(seen, false);
        });
        tasks.push(task);
    }

    // await tasks to propagate panics
    for task in tasks {
        task.await.unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
#[serial]
async fn broadcast_delete_does_not_affect_other_users() {
    init_env();

    // after producing and fetching undelivered notifications
    // when any user deletes notification it should not affect other users

    let client = Client::new();
    let users = (0..10).map(|_| create_consumer_jwt()).collect::<Vec<_>>();
    let producer = create_producer_jwt();

    // create notification
    let response = client
        .post(format!(
            "http://{}/api/v1/notifications/undelivered",
            address()
        ))
        .bearer_auth(producer)
        .header(CONTENT_TYPE, "application/json")
        .body(
            json!({
                "invalidate_at": OffsetDateTime::now_utc() + Duration::from_secs(30),
                "user_ids": [],
                "producer_notification_id": 1,
                "content_type": "utf-8",
                "content": "RG9uJ3QgeW91IGRhcmUgZGVjb2RpbmcgbWUh"
            })
            .to_string(),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let response_body = response.bytes().await.unwrap();
    let response_body = serde_json::from_slice::<Value>(&response_body).unwrap();
    let saved_notification_id = response_body
        .get("id")
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let mut tasks = Vec::with_capacity(users.len());

    // fetch undelivered notifications for all users
    for user in users.iter() {
        let client_clone = client.clone();
        let user_clone = user.clone();
        let task = tokio::spawn(async move {
            let response = client_clone
                .get(format!(
                    "http://{}/api/v1/notifications/undelivered",
                    address()
                ))
                .bearer_auth(&user_clone)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
        tasks.push(task);
    }

    // await tasks to propagate panics
    while !tasks.is_empty() {
        let task = tasks.pop().unwrap();
        task.await.unwrap();
    }

    // users[0] deletes notification
    let response = client
        .delete(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&users[0])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    // fetch notification for users[0]
    // it should not exist
    let response = client
        .get(format!(
            "http://{}/api/v1/notifications/delivered/{}",
            address(),
            saved_notification_id
        ))
        .bearer_auth(&users[0])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // fetch notification for rest of the users
    // notification should still exist
    for user in &users[1..] {
        let client_clone = client.clone();
        let user_clone = user.clone();
        let saved_notification_id_clone = saved_notification_id.clone();
        let task = tokio::spawn(async move {
            let response = client_clone
                .get(format!(
                    "http://{}/api/v1/notifications/delivered/{}",
                    address(),
                    saved_notification_id_clone
                ))
                .bearer_auth(&user_clone)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
        });
        tasks.push(task);
    }

    // await tasks to propagate panics
    for task in tasks {
        task.await.unwrap();
    }
}
