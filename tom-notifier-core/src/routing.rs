use crate::{
    application::ApplicationState,
    auth::{require_all_roles, Role, User},
    dto::{input, output},
    error::Error,
    service::notifications_service::NotificationsService,
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post, put},
    Extension, Json, Router,
};
use bson::oid::ObjectId;
use std::sync::Arc;

pub fn routing() -> Router<ApplicationState> {
    Router::new()
        .route(
            "/api/v1/notifications/undelivered",
            post(post_notifications_undelivered).get(get_notifications_undelivered),
        )
        .route(
            "/api/v1/notifications/undelivered/:notification_id/invalidate_at",
            put(put_notifications_undelivered_invalidate_at),
        )
        .route(
            "/api/v1/notifications/delivered",
            get(get_notifications_delivered),
        )
        .route(
            "/api/v1/notifications/delivered/:notification_id",
            get(get_notification_delivered).delete(delete_notification_delivered),
        )
        .route(
            "/api/v1/notifications/delivered/:notification_id/seen",
            put(put_notification_delivered_seen),
        )
}

///
/// Create new notification
///
/// ### Returns
/// 200 on success
///
/// ### Errors
/// - 400 payload is invalid when content is not valid base64
/// - 403 when user lacks role [Role::ProduceNotifications]
/// - 409 notification with producer_notification_id was already created by the user
/// - 413 when content is too large
/// - 422 when invalidate_at is set to past date
///
async fn post_notifications_undelivered(
    State(notifications_service): State<Arc<dyn NotificationsService>>,
    Extension(user): Extension<User>,
    Json(notification): Json<input::Notification>,
) -> Result<(StatusCode, Json<output::NotificationId>), Error> {
    require_all_roles(&user, &[Role::ProduceNotifications])?;

    let notification_id = notifications_service
        .save_notification(user.id, notification.into())
        .await?;

    Ok((StatusCode::OK, Json(notification_id)))
}

///
/// Find notifications that have not yet been delivered.
/// Skip notifications that have been invalided.
///
/// Since this endpoint delivers notifications, they are marked
/// as delivered. So fetching data multiple times will yield
/// different results.
///
/// This endpoint is meant for long polling new notifications
///
/// ### Returns
/// 200 on success
///
async fn get_notifications_undelivered(
    State(notifications_service): State<Arc<dyn NotificationsService>>,
    Extension(user): Extension<User>,
) -> Result<(StatusCode, Json<Vec<output::Notification>>), Error> {
    let notifications = notifications_service
        .find_undelivered_notifications(user.id)
        .await?;

    Ok((StatusCode::OK, Json(notifications)))
}

///
/// Update invalidate_at property of the notification
///
/// ### Returns
/// 204 on success
///
/// ### Errors
/// - 403 when user does not have role [Role::ProduceNotifications]
/// - 404 when
///     - notification with id does not exist
///     - notification was not produced by the producer
/// - 422 when invalidate_at is set to datetime that have already passed
///
async fn put_notifications_undelivered_invalidate_at(
    State(notifications_service): State<Arc<dyn NotificationsService>>,
    Extension(user): Extension<User>,
    Path(id): Path<ObjectId>,
    Json(invalidate_at): Json<input::NotificationInvalidateAt>,
) -> Result<StatusCode, Error> {
    require_all_roles(&user, &[Role::ProduceNotifications])?;

    notifications_service
        .update_notification_invalidate_at(id, user.id, invalidate_at)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

///
/// Find notifications that have already been delivered
///
/// ### Returns
/// 200 on success
///
async fn get_notifications_delivered(
    State(notifications_service): State<Arc<dyn NotificationsService>>,
    Extension(user): Extension<User>,
    Query(pagination): Query<input::Pagination>,
    Query(filters): Query<input::NotificationFilters>,
) -> Result<(StatusCode, Json<Vec<output::Notification>>), Error> {
    let notifications = notifications_service
        .find_delivered_notifications(user.id, pagination, filters)
        .await?;

    Ok((StatusCode::OK, Json(notifications)))
}

///
/// Find notification
///
/// ### Returns
/// 200 on success
///
/// ### Errors
/// - 404 when
///     - notification does not exist
///     - user does not belong to notification recipients
///     - notification have not been delivered yet
///     - notification is deleted
///
async fn get_notification_delivered(
    State(notifications_service): State<Arc<dyn NotificationsService>>,
    Extension(user): Extension<User>,
    Path(id): Path<ObjectId>,
) -> Result<(StatusCode, Json<output::Notification>), Error> {
    let notification = notifications_service
        .find_delivered_notification(id, user.id)
        .await?;

    Ok((StatusCode::OK, Json(notification)))
}

///
/// Delete notification
///
/// ### Returns
/// 204 on success
///
/// ### Errors
/// - 404 when
///     - notification does not exist
///     - user does not belong to notification recipients
///     - notification have not been delivered yet
///     - notification is deleted
///
async fn delete_notification_delivered(
    State(notifications_service): State<Arc<dyn NotificationsService>>,
    Extension(user): Extension<User>,
    Path(id): Path<ObjectId>,
) -> Result<StatusCode, Error> {
    notifications_service
        .delete_notification(id, user.id)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

///
/// Update seen property of notification
///
/// ### Returns
/// 204 on success
///
/// ### Errors
/// - 404 when
///     - notification does not exist
///     - user does not belong to notification recipients
///     - notification have not been delivered yet
///     - notification is deleted
///
async fn put_notification_delivered_seen(
    State(notifications_service): State<Arc<dyn NotificationsService>>,
    Extension(user): Extension<User>,
    Path(id): Path<ObjectId>,
    Json(seen): Json<input::NotificationSeen>,
) -> Result<StatusCode, Error> {
    notifications_service
        .update_notification_seen(id, user.id, seen)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        error::Error, repository, service::notifications_service::MockNotificationsService,
    };
    use axum::{
        body::Body,
        http::{header::CONTENT_TYPE, Method, Request},
    };
    use serde_json::json;
    use std::time::Duration;
    use time::{macros::datetime, OffsetDateTime};
    use tower::ServiceExt;
    use uuid::Uuid;

    fn create_consumer() -> User {
        User::new(Uuid::new_v4(), vec![])
    }

    fn create_producer() -> User {
        User::new(
            Uuid::new_v4(),
            vec![Role::ProduceNotifications.as_ref().to_string()],
        )
    }

    fn mock_application_state() -> ApplicationState {
        ApplicationState {
            notifications_service: Arc::new(MockNotificationsService::new()),
        }
    }

    #[tokio::test]
    async fn post_notifications_undelivered_missing_role() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_save_notification()
            .returning(|_, _| {
                Ok(output::NotificationId {
                    id: "some id".to_string(),
                })
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/notifications/undelivered")
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_consumer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>,
                            "user_ids": Vec::<Uuid>::new(),
                            "producer_notification_id": 1,
                            "content_type": "utf-8",
                            "content": "VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==",
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn post_notifications_undelivered_validation_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_save_notification()
            .returning(|_, _| Err(Error::Validation("invalidate_at passed")));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/notifications/undelivered")
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>,
                            "user_ids": Vec::<Uuid>::new(),
                            "producer_notification_id": 1,
                            "content_type": "utf-8",
                            "content": "VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==",
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn post_notifications_undelivered_validation_notification_too_large_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_save_notification()
            .returning(|_, _| {
                Err(Error::ValidationNotificationTooLarge {
                    size: 4096,
                    max_size: 1024,
                })
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/notifications/undelivered")
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>,
                            "user_ids": Vec::<Uuid>::new(),
                            "producer_notification_id": 1,
                            "content_type": "utf-8",
                            "content": "VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==",
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn post_notifications_undelivered_already_exist() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_save_notification()
            .returning(|_, _| Err(Error::NotificationAlreadySaved));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/notifications/undelivered")
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>,
                            "user_ids": Vec::<Uuid>::new(),
                            "producer_notification_id": 1,
                            "content_type": "utf-8",
                            "content": "VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==",
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn post_notifications_undelivered_database_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_save_notification()
            .returning(|_, _| {
                Err(Error::Database(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("unexpected database error")).into(),
                )))
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/notifications/undelivered")
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>,
                            "user_ids": Vec::<Uuid>::new(),
                            "producer_notification_id": 1,
                            "content_type": "utf-8",
                            "content": "VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==",
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn post_notifications_undelivered_success_code() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_save_notification()
            .returning(|_, _| {
                Ok(output::NotificationId {
                    id: "some id".to_string(),
                })
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/notifications/undelivered")
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>,
                            "user_ids": Vec::<Uuid>::new(),
                            "producer_notification_id": 1,
                            "content_type": "utf-8",
                            "content": "VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==",
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_notifications_undelivered_database_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_find_undelivered_notifications()
            .returning(|_| {
                Err(Error::Database(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                )))
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/notifications/undelivered")
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn get_notifications_undelivered_success_code() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_find_undelivered_notifications()
            .returning(|_| Ok(vec![]));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/notifications/undelivered")
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn put_notifications_undelivered_invalidate_at_validation_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_invalidate_at()
            .returning(|_, _, _| Err(Error::Validation("invalidate at passed")));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/undelivered/{}/invalidate_at",
                        ObjectId::new()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": OffsetDateTime::now_utc() - Duration::from_secs(300),
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn put_notifications_undelivered_invalidate_at_missing_role() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_invalidate_at()
            .returning(|_, _, _| Ok(()));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/undelivered/{}/invalidate_at",
                        ObjectId::new()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_consumer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn put_notifications_undelivered_invalidate_at_notification_not_exist() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_invalidate_at()
            .returning(|_, _, _| Err(Error::NotificationNotExist));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/undelivered/{}/invalidate_at",
                        ObjectId::new()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_notifications_undelivered_invalidate_at_database_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_invalidate_at()
            .returning(|_, _, _| {
                Err(Error::Database(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                )))
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/undelivered/{}/invalidate_at",
                        ObjectId::new()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn put_notifications_undelivered_invalidate_at_success_code() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_invalidate_at()
            .returning(|_, _, _| Ok(()));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/undelivered/{}/invalidate_at",
                        ObjectId::new()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_producer())
                    .body(
                        json!({
                            "invalidate_at": None as Option<OffsetDateTime>
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn get_notifications_delivered_database_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_find_delivered_notifications()
            .returning(|_, _, _| {
                Err(Error::Database(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                )))
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/notifications/delivered?page_idx=5&page_size=10")
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn get_notifications_delivered_success_code() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_find_delivered_notifications()
            .returning(|_, _, _| Ok(vec![]));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/api/v1/notifications/delivered?page_idx=5&page_size=10&seen=true")
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_notification_delivered_not_exist() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_find_delivered_notification()
            .returning(|_, _| Err(Error::NotificationNotExist));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}",
                        ObjectId::new().to_hex()
                    ))
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_notification_delivered_database_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_find_delivered_notification()
            .returning(|_, _| {
                Err(Error::Database(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                )))
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}",
                        ObjectId::new().to_hex()
                    ))
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn get_notification_delivered_success_code() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_find_delivered_notification()
            .returning(|_, _| {
                Ok(output::Notification {
                    id: ObjectId::new().to_hex(),
                    created_at: datetime!(2024-02-13 19:09:35 UTC),
                    created_by: Uuid::new_v4(),
                    seen: true,
                    content_type: "utf-8".to_string(),
                    content: b"some content".to_vec(),
                })
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}",
                        ObjectId::new().to_hex()
                    ))
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn delete_notification_delivered_notification_not_exist() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_delete_notification()
            .returning(|_, _| Err(Error::NotificationNotExist));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}",
                        ObjectId::new().to_hex()
                    ))
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn delete_notification_delivered_database_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_delete_notification()
            .returning(|_, _| {
                Err(Error::Database(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                )))
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}",
                        ObjectId::new().to_hex()
                    ))
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn delete_notification_delivered_success_code() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_delete_notification()
            .returning(|_, _| Ok(()));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}",
                        ObjectId::new().to_hex()
                    ))
                    .extension(create_consumer())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn put_notification_delivered_seen_notification_not_exist() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_seen()
            .returning(|_, _, _| Err(Error::NotificationNotExist));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}/seen",
                        ObjectId::new().to_hex()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_consumer())
                    .body(
                        json!({
                            "seen": false,
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn put_notification_delivered_seen_database_error() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_seen()
            .returning(|_, _, _| {
                Err(Error::Database(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                )))
            });

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}/seen",
                        ObjectId::new().to_hex()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_consumer())
                    .body(
                        json!({
                            "seen": false
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn put_notification_delivered_seen_success_code() {
        let mut notifications_service = MockNotificationsService::new();
        notifications_service
            .expect_update_notification_seen()
            .returning(|_, _, _| Ok(()));

        let mut application_state = mock_application_state();
        application_state.notifications_service = Arc::new(notifications_service);

        let response = routing()
            .with_state(application_state)
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!(
                        "/api/v1/notifications/delivered/{}/seen",
                        ObjectId::new().to_hex()
                    ))
                    .header(CONTENT_TYPE, "application/json")
                    .extension(create_consumer())
                    .body(
                        json!({
                            "seen": false,
                        })
                        .to_string(),
                    )
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
