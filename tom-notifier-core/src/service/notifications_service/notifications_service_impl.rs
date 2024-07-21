use super::{NotificationsService, NotificationsServiceConfig};
use crate::{
    dto::{input, output},
    error::Error,
    repository::{self, NotificationsRepository},
};
use axum::async_trait;
use bson::oid::ObjectId;
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

pub struct NotificationsServiceImpl {
    config: NotificationsServiceConfig,
    repository: Arc<dyn NotificationsRepository>,
}

impl NotificationsServiceImpl {
    pub fn new(
        config: NotificationsServiceConfig,
        repository: Arc<dyn NotificationsRepository>,
    ) -> Self {
        Self { config, repository }
    }

    fn validate_save_notification(&self, notification: &input::Notification) -> Result<(), Error> {
        Self::validate_invalidate_at_not_passed(&notification.invalidate_at)?;
        self.validate_content_not_too_long(&notification.content)?;

        Ok(())
    }

    fn validate_update_invalidate_at(
        invalidate_at: &input::NotificationInvalidateAt,
    ) -> Result<(), Error> {
        Self::validate_invalidate_at_not_passed(&invalidate_at.invalidate_at)?;

        Ok(())
    }

    fn validate_invalidate_at_not_passed(
        invalidate_at: &Option<OffsetDateTime>,
    ) -> Result<(), Error> {
        if let Some(invalidate_at) = invalidate_at {
            if *invalidate_at <= OffsetDateTime::now_utc() {
                return Err(Error::Validation("invalidate_at already passed"));
            }
        }

        Ok(())
    }

    fn validate_content_not_too_long(&self, content: &Vec<u8>) -> Result<(), Error> {
        if content.len() > self.config.max_content_len {
            return Err(Error::ValidationNotificationTooLarge {
                size: content.len(),
                max_size: self.config.max_content_len,
            });
        }

        Ok(())
    }
}

#[async_trait]
impl NotificationsService for NotificationsServiceImpl {
    async fn save_notification(
        &self,
        producer_id: Uuid,
        notification: input::Notification,
    ) -> Result<output::NotificationId, Error> {
        tracing::info!("creating notification");
        tracing::trace!(?notification);

        self.validate_save_notification(&notification)?;

        let inserted_notification = self
            .repository
            .insert(
                notification.user_ids,
                OffsetDateTime::now_utc(),
                notification.invalidate_at,
                producer_id,
                notification.producer_notification_id,
                notification.content_type,
                notification.content,
            )
            .await
            .map_err(|err| match err {
                repository::Error::InsertUniqueViolation => Error::NotificationAlreadySaved,
                err => Error::Database(err),
            })?;

        let id = inserted_notification.id.to_hex();
        tracing::info!(id, "created notification");

        // TODO: implement sending to fanout service

        Ok(output::NotificationId { id })
    }

    async fn find_undelivered_notifications(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<output::Notification>, Error> {
        tracing::info!("finding undelivered notifications");

        let notifications = self.repository.find_many_undelivered(user_id).await?;
        tracing::info!(count = notifications.len(), "found notifications");

        if !notifications.is_empty() {
            let notifications_ids = notifications
                .iter()
                .map(|notification| notification.id)
                .collect::<Vec<_>>();

            tracing::info!(?notifications_ids, "inserting confirmations");
            self.repository
                .insert_many_confirmations(&notifications_ids, user_id)
                .await?;
            tracing::info!("inserted confirmations");
        }

        let notifications = notifications
            .into_iter()
            .map(output::Notification::from)
            .collect();

        Ok(notifications)
    }

    async fn find_delivered_notifications(
        &self,
        user_id: Uuid,
        pagination: input::Pagination,
        filters: input::NotificationFilters,
    ) -> Result<Vec<output::Notification>, Error> {
        tracing::info!("finding delivered notifications");
        tracing::trace!(?filters);

        let notifications = self
            .repository
            .find_many_delivered(user_id, pagination, filters)
            .await?;
        tracing::info!(count = notifications.len(), "found notifications");

        let notifications = notifications
            .into_iter()
            .map(output::Notification::from)
            .collect();

        Ok(notifications)
    }

    async fn find_delivered_notification(
        &self,
        id: ObjectId,
        user_id: Uuid,
    ) -> Result<output::Notification, Error> {
        tracing::info!("finding notification");

        let notification = self
            .repository
            .find_delivered(id, user_id)
            .await?
            .ok_or(Error::NotificationNotExist)?;

        tracing::info!("found notification");

        Ok(notification.into())
    }

    async fn delete_notification(&self, id: ObjectId, user_id: Uuid) -> Result<(), Error> {
        tracing::info!("deleting notification");

        self.repository
            .delete(id, user_id)
            .await
            .map_err(|err| match err {
                repository::Error::NoDocumentUpdated => Error::NotificationNotExist,
                err => Error::Database(err),
            })?;

        tracing::info!("deleted notification");

        // TODO: implement sending to fanout service

        Ok(())
    }

    async fn update_notification_invalidate_at(
        &self,
        id: ObjectId,
        producer_id: Uuid,
        invalidate_at: input::NotificationInvalidateAt,
    ) -> Result<(), Error> {
        tracing::info!("updating invalidate_at");
        tracing::trace!(?invalidate_at);

        Self::validate_update_invalidate_at(&invalidate_at)?;

        let update_result = self
            .repository
            .update_invalidate_at(id, producer_id, invalidate_at.invalidate_at)
            .await;

        match update_result {
            Ok(()) => {
                tracing::info!("updated invalidate_at");
                Ok(())
            }
            Err(repository::Error::NoDocumentUpdated) => Err(Error::NotificationNotExist),
            Err(err) => Err(Error::Database(err)),
        }
    }

    async fn update_notification_seen(
        &self,
        id: ObjectId,
        user_id: Uuid,
        seen: input::NotificationSeen,
    ) -> Result<(), Error> {
        tracing::info!("updating seen");
        tracing::trace!(?seen);

        let input::NotificationSeen { seen } = seen;

        self.repository
            .update_confirmation_seen(id, user_id, seen)
            .await
            .map_err(|err| match err {
                repository::Error::NoDocumentUpdated => Error::NotificationNotExist,
                err => Error::Database(err),
            })?;

        tracing::info!("updated seen");

        // TODO: implement sending to fanout service

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bson::oid::ObjectId;
    use repository::{InsertedNotification, MockNotificationsRepository};
    use std::time::Duration;
    use time::macros::datetime;

    #[tokio::test]
    async fn save_notification_validation_invalidate_at_none_ok() {
        let invalidate_at = None;
        let invalidate_at_clone = invalidate_at.clone();

        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_insert()
            .return_once(move |_, _, _, _, _, _, _| {
                Ok(InsertedNotification {
                    id: ObjectId::new(),
                    created_at: OffsetDateTime::now_utc(),
                    invalidate_at: invalidate_at_clone,
                    user_ids: vec![],
                    producer_id: Uuid::new_v4(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"data".to_vec(),
                })
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let save_result = service
            .save_notification(
                Uuid::from_u128(5890123809123),
                input::Notification {
                    invalidate_at,
                    user_ids: Vec::new(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==".to_vec(),
                },
            )
            .await;

        assert!(save_result.is_ok());
    }

    #[tokio::test]
    async fn save_notification_validation_invalidate_at_some_ok() {
        let invalidate_at = OffsetDateTime::now_utc() + Duration::from_secs(600);
        assert!(invalidate_at > OffsetDateTime::now_utc());
        let invalidate_at = Some(invalidate_at);
        let invalidate_at_clone = invalidate_at.clone();

        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_insert()
            .return_once(move |_, _, _, _, _, _, _| {
                Ok(InsertedNotification {
                    id: ObjectId::new(),
                    created_at: OffsetDateTime::now_utc(),
                    invalidate_at: invalidate_at_clone,
                    user_ids: vec![],
                    producer_id: Uuid::new_v4(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"data".to_vec(),
                })
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let save_result = service
            .save_notification(
                Uuid::from_u128(5890123809123),
                input::Notification {
                    invalidate_at,
                    user_ids: Vec::new(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==".to_vec(),
                },
            )
            .await;

        assert!(save_result.is_ok());
    }

    #[tokio::test]
    async fn save_notification_validation_invalidate_at_err() {
        let invalidate_at = OffsetDateTime::now_utc() - Duration::from_secs(600);
        assert!(invalidate_at < OffsetDateTime::now_utc());
        let invalidate_at = Some(invalidate_at);
        let invalidate_at_clone = invalidate_at.clone();

        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_insert()
            .return_once(move |_, _, _, _, _, _, _| {
                Ok(InsertedNotification {
                    id: ObjectId::new(),
                    created_at: OffsetDateTime::now_utc(),
                    invalidate_at: invalidate_at_clone,
                    user_ids: vec![],
                    producer_id: Uuid::new_v4(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"data".to_vec(),
                })
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let save_result = service
            .save_notification(
                Uuid::from_u128(5890123809123),
                input::Notification {
                    invalidate_at,
                    user_ids: Vec::new(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==".to_vec(),
                },
            )
            .await;

        assert!(matches!(save_result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn save_notification_validation_content_length_ok() {
        const MAX_CONTENT_LEN: usize = 8;

        let content = b"MTIzNA==".to_vec();
        let content_clone = content.clone();
        assert!(content.len() <= MAX_CONTENT_LEN);

        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_insert()
            .return_once(move |_, _, _, _, _, _, _| {
                Ok(InsertedNotification {
                    id: ObjectId::new(),
                    created_at: OffsetDateTime::now_utc(),
                    invalidate_at: None,
                    user_ids: vec![],
                    producer_id: Uuid::new_v4(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: content_clone,
                })
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: MAX_CONTENT_LEN,
            },
            Arc::new(repository),
        );

        let save_result = service
            .save_notification(
                Uuid::from_u128(5890123809123),
                input::Notification {
                    invalidate_at: None,
                    user_ids: Vec::new(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content,
                },
            )
            .await;

        assert!(save_result.is_ok());
    }

    #[tokio::test]
    async fn save_notification_validation_content_length_err() {
        const MAX_CONTENT_LEN: usize = 8;

        let content = b"VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==".to_vec();
        let content_clone = content.clone();
        assert!(content.len() > MAX_CONTENT_LEN);

        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_insert()
            .return_once(move |_, _, _, _, _, _, _| {
                Ok(InsertedNotification {
                    id: ObjectId::new(),
                    created_at: OffsetDateTime::now_utc(),
                    invalidate_at: None,
                    user_ids: vec![],
                    producer_id: Uuid::new_v4(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: content_clone,
                })
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: MAX_CONTENT_LEN,
            },
            Arc::new(repository),
        );

        let save_result = service
            .save_notification(
                Uuid::from_u128(5890123809123),
                input::Notification {
                    invalidate_at: None,
                    user_ids: Vec::new(),
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content,
                },
            )
            .await;

        assert!(matches!(
            save_result,
            Err(Error::ValidationNotificationTooLarge {
                size: _,
                max_size: _
            })
        ));
    }

    #[tokio::test]
    async fn save_notification_already_saved() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_insert()
            .returning(|_, _, _, _, _, _, _| Err(repository::Error::InsertUniqueViolation));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let save_result = service
            .save_notification(
                Uuid::from_u128(12371928379128),
                input::Notification {
                    invalidate_at: None,
                    user_ids: vec![],
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==".to_vec(),
                },
            )
            .await;

        assert!(matches!(save_result, Err(Error::NotificationAlreadySaved)));
    }

    #[tokio::test]
    async fn save_notification_database_error() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_insert().returning(|_, _, _, _, _, _, _| {
            Err(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("unexpected database error")).into(),
            ))
        });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let save_result = service
            .save_notification(
                Uuid::from_u128(12371928379128),
                input::Notification {
                    invalidate_at: None,
                    user_ids: vec![],
                    producer_notification_id: 1,
                    content_type: "utf-8".to_string(),
                    content: b"VGhpcyBjb25lbnQgaXMgbm90IGltcG9ydGFudA==".to_vec(),
                },
            )
            .await;

        assert!(matches!(save_result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn find_undelivered_notifications_database_error() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_find_many_undelivered().returning(|_| {
            Err(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            ))
        });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .find_undelivered_notifications(Uuid::from_u128(124801283012))
            .await;

        assert!(matches!(result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn find_undelivered_notifications_insert_confirmations_error() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_find_many_undelivered().returning(|_| {
            let notifications = vec![repository::Notification {
                id: ObjectId::new(),
                created_at: datetime!(2024-02-12 18:57:00 UTC),
                seen: false,
                content_type: "utf-8".to_string(),
                content: b"It's just a mock notification".to_vec(),
            }];
            Ok(notifications)
        });
        repository
            .expect_insert_many_confirmations()
            .returning(|_, _| {
                Err(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                ))
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .find_undelivered_notifications(Uuid::from_u128(124801283012))
            .await;

        assert!(matches!(result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn find_undelivered_notifications_no_notifications() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_find_many_undelivered()
            .returning(|_| Ok(vec![]));
        repository.expect_insert_many_confirmations().never(); // makes sure method is not called when there's no notifications
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .find_undelivered_notifications(Uuid::from_u128(124801283012))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn find_undelivered_notifications_ok() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_find_many_undelivered().returning(|_| {
            Ok(vec![
                repository::Notification {
                    id: ObjectId::new(),
                    created_at: OffsetDateTime::now_utc(),
                    seen: false,
                    content_type: "utf-8".to_string(),
                    content: b"abc".to_vec(),
                },
                repository::Notification {
                    id: ObjectId::new(),
                    created_at: OffsetDateTime::now_utc(),
                    seen: false,
                    content_type: "utf-8".to_string(),
                    content: b"abc2".to_vec(),
                },
            ])
        });
        repository
            .expect_insert_many_confirmations()
            .returning(|_, _| Ok(()));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let notifications = service
            .find_undelivered_notifications(Uuid::from_u128(124801283012))
            .await
            .unwrap();

        assert_eq!(notifications.len(), 2);
    }

    #[tokio::test]
    async fn find_delivered_notifications_database_error() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_find_many_delivered()
            .returning(|_, _, _| {
                Err(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                ))
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let find_result = service
            .find_delivered_notifications(
                Uuid::from_u128(58190832021938),
                input::Pagination {
                    page_idx: 0,
                    page_size: u32::MAX,
                },
                input::NotificationFilters { seen: None },
            )
            .await;

        assert!(matches!(find_result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn find_delivered_notifications_ok() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_find_many_delivered()
            .returning(|_, _, _| {
                Ok(vec![
                    repository::Notification {
                        id: ObjectId::new(),
                        created_at: OffsetDateTime::now_utc(),
                        seen: false,
                        content_type: "utf-8".to_string(),
                        content: b"abc".to_vec(),
                    },
                    repository::Notification {
                        id: ObjectId::new(),
                        created_at: OffsetDateTime::now_utc(),
                        seen: false,
                        content_type: "utf-8".to_string(),
                        content: b"abc2".to_vec(),
                    },
                ])
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let notifications = service
            .find_delivered_notifications(
                Uuid::from_u128(58190832021938),
                input::Pagination {
                    page_idx: 0,
                    page_size: u32::MAX,
                },
                input::NotificationFilters { seen: None },
            )
            .await
            .unwrap();

        assert_eq!(notifications.len(), 2);
    }

    #[tokio::test]
    async fn find_delivered_notification_not_found() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_find_delivered()
            .returning(|_, _| Ok(None));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let find_result = service
            .find_delivered_notification(ObjectId::new(), Uuid::from_u128(75098123))
            .await;

        assert!(matches!(find_result, Err(Error::NotificationNotExist)));
    }

    #[tokio::test]
    async fn find_delivered_notification_database_error() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_find_delivered().returning(|_, _| {
            Err(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            ))
        });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let find_result = service
            .find_delivered_notification(ObjectId::new(), Uuid::from_u128(75098123))
            .await;

        assert!(matches!(find_result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn find_delivered_notification_ok() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_find_delivered().returning(|_, _| {
            Ok(Some(repository::Notification {
                id: ObjectId::new(),
                created_at: OffsetDateTime::now_utc(),
                seen: false,
                content_type: "utf-8".to_string(),
                content: b"abc".to_vec(),
            }))
        });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let find_result = service
            .find_delivered_notification(ObjectId::new(), Uuid::from_u128(75098123))
            .await;

        assert!(find_result.is_ok());
    }

    #[tokio::test]
    async fn delete_notification_no_document_updated() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_delete()
            .returning(|_, _| Err(repository::Error::NoDocumentUpdated));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .delete_notification(ObjectId::new(), Uuid::from_u128(31985781293))
            .await;

        assert!(matches!(result, Err(Error::NotificationNotExist)));
    }

    #[tokio::test]
    async fn delete_notification_database_error() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_delete().returning(|_, _| {
            Err(repository::Error::Mongo(
                mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
            ))
        });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .delete_notification(ObjectId::new(), Uuid::from_u128(31985781293))
            .await;

        assert!(matches!(result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn delete_notification_ok() {
        let mut repository = MockNotificationsRepository::new();
        repository.expect_delete().returning(|_, _| Ok(()));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .delete_notification(ObjectId::new(), Uuid::from_u128(31985781293))
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn update_notification_invalidate_at_invalidate_at_passed() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_update_invalidate_at()
            .returning(|_, _, _| Ok(()));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let passed_invalidate_at = OffsetDateTime::now_utc() - Duration::from_secs(300);

        let result = service
            .update_notification_invalidate_at(
                ObjectId::new(),
                Uuid::from_u128(5019283019283),
                input::NotificationInvalidateAt {
                    invalidate_at: Some(passed_invalidate_at),
                },
            )
            .await;

        assert!(matches!(result, Err(Error::Validation(_))));
    }

    #[tokio::test]
    async fn update_notification_invalidate_at_no_document_updated() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_update_invalidate_at()
            .returning(|_, _, _| Err(repository::Error::NoDocumentUpdated));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .update_notification_invalidate_at(
                ObjectId::new(),
                Uuid::from_u128(5019283019283),
                input::NotificationInvalidateAt {
                    invalidate_at: None,
                },
            )
            .await;

        assert!(matches!(result, Err(Error::NotificationNotExist)));
    }

    #[tokio::test]
    async fn update_notification_invalidate_at_database_error() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_update_invalidate_at()
            .returning(|_, _, _| {
                Err(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                ))
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .update_notification_invalidate_at(
                ObjectId::new(),
                Uuid::from_u128(5019283019283),
                input::NotificationInvalidateAt {
                    invalidate_at: None,
                },
            )
            .await;

        assert!(matches!(result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn update_notification_invalidate_at_ok() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_update_invalidate_at()
            .returning(|_, _, _| Ok(()));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .update_notification_invalidate_at(
                ObjectId::new(),
                Uuid::from_u128(5019283019283),
                input::NotificationInvalidateAt {
                    invalidate_at: None,
                },
            )
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn update_notification_seen_no_document_updated() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_update_confirmation_seen()
            .returning(|_, _, _| Err(repository::Error::NoDocumentUpdated));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .update_notification_seen(
                ObjectId::new(),
                Uuid::from_u128(5801283120),
                input::NotificationSeen { seen: true },
            )
            .await;

        assert!(matches!(result, Err(Error::NotificationNotExist)));
    }

    #[tokio::test]
    async fn update_notification_seen_database_error() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_update_confirmation_seen()
            .returning(|_, _, _| {
                Err(repository::Error::Mongo(
                    mongodb::error::ErrorKind::Custom(Arc::new("any database error")).into(),
                ))
            });
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .update_notification_seen(
                ObjectId::new(),
                Uuid::from_u128(5801283120),
                input::NotificationSeen { seen: true },
            )
            .await;

        assert!(matches!(result, Err(Error::Database(_))));
    }

    #[tokio::test]
    async fn update_notification_seen_ok() {
        let mut repository = MockNotificationsRepository::new();
        repository
            .expect_update_confirmation_seen()
            .returning(|_, _, _| Ok(()));
        let service = NotificationsServiceImpl::new(
            NotificationsServiceConfig {
                max_content_len: usize::MAX,
            },
            Arc::new(repository),
        );

        let result = service
            .update_notification_seen(
                ObjectId::new(),
                Uuid::from_u128(5801283120),
                input::NotificationSeen { seen: true },
            )
            .await;

        assert!(result.is_ok());
    }
}
