use super::{
    dto::NotificationsDeduplicationServiceConfig,
    notifications_deduplication_service_garbage_collector::NotificationsDeduplicationServiceGarbageCollector,
    NotificationStatusUpdate, NotificationsDeduplicationService,
};
use crate::error::Error;
use axum::async_trait;
use bson::oid::ObjectId;
use std::{collections::HashMap, sync::Arc};
use time::OffsetDateTime;
use tokio::{
    sync::{Mutex, Notify},
    task::JoinHandle,
};

pub struct NotificationsDeduplicationServiceImpl {
    last_updates: Arc<Mutex<HashMap<ObjectId, OffsetDateTime>>>,

    garbage_collector_task: JoinHandle<()>,
    garbage_collector_close_notify: Arc<Notify>,
}

impl NotificationsDeduplicationServiceImpl {
    pub fn new(config: NotificationsDeduplicationServiceConfig) -> Self {
        let last_updates = HashMap::new();
        let last_updates = Mutex::new(last_updates);
        let last_updates = Arc::new(last_updates);

        let garbage_collector = NotificationsDeduplicationServiceGarbageCollector::new(
            config,
            Arc::clone(&last_updates),
        );

        let close_notify = Notify::new();
        let close_notify = Arc::new(close_notify);

        let close_notify_clone = Arc::clone(&close_notify);
        let garbage_collector_task = tokio::spawn(async move {
            tracing::info!("notifications deduplication service garbage collector started");
            garbage_collector.run(close_notify_clone).await;
            tracing::info!("notifications deduplication service garbage collector finished");
        });

        Self {
            last_updates,
            garbage_collector_task,
            garbage_collector_close_notify: close_notify,
        }
    }

    pub async fn close(self) {
        self.garbage_collector_close_notify.notify_one();
        if let Err(err) = self.garbage_collector_task.await {
            // This should never happen
            tracing::error!(%err, "garbage collector task failed");
        }
    }
}

#[async_trait]
impl NotificationsDeduplicationService for NotificationsDeduplicationServiceImpl {
    ///
    /// Function takes [NotificationStatusUpdate] and makes sure it is not a duplicate.
    ///
    /// ### Errors
    /// - [Error::Duplicate] when
    ///     - this [NotificationStatusUpdate] have already been deduplicated
    ///     - [NotificationStatusUpdate] with the same id and more recent timestamp was deduplicated
    ///
    #[tracing::instrument(
        name = "Deduplication"
        skip_all,
        fields(
            id = notification.id.to_hex(),
            timestamp = %notification.timestamp,
        )
    )]
    async fn deduplicate(&self, notification: NotificationStatusUpdate) -> Result<(), Error> {
        tracing::trace!("deduplicating notification status update");

        let mut last_updates = self.last_updates.lock().await;

        match last_updates.get_mut(&notification.id) {
            Some(datetime) if *datetime < notification.timestamp => {
                *datetime = notification.timestamp;
                tracing::trace!("replaced previous notification status update");
                Ok(())
            }
            Some(_) => Err(Error::Duplicate),
            None => {
                last_updates.insert(notification.id, notification.timestamp);
                tracing::trace!("first notification status update");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::time::Duration;
    use tokio::time::sleep;

    #[tokio::test]
    async fn deduplicate_first_entry() {
        let config = NotificationsDeduplicationServiceConfig {
            notification_lifespan: Duration::from_secs(300),
            garbage_collector_interval: Duration::from_secs(300),
        };
        let service = NotificationsDeduplicationServiceImpl::new(config);

        let now = OffsetDateTime::now_utc();
        let notification = NotificationStatusUpdate {
            id: ObjectId::new(),
            timestamp: now,
        };

        let result = service.deduplicate(notification).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn deduplicate_timestamp_after_previous() {
        let config = NotificationsDeduplicationServiceConfig {
            notification_lifespan: Duration::from_secs(300),
            garbage_collector_interval: Duration::from_secs(300),
        };
        let service = NotificationsDeduplicationServiceImpl::new(config);

        let id = ObjectId::new();
        let datetime = OffsetDateTime::now_utc();
        {
            service.last_updates.lock().await.insert(id, datetime);
        }

        let notification = NotificationStatusUpdate {
            id,
            timestamp: datetime + Duration::from_secs(30),
        };

        let result = service.deduplicate(notification).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn deduplicate_timestamp_before_previous() {
        let config = NotificationsDeduplicationServiceConfig {
            notification_lifespan: Duration::from_secs(300),
            garbage_collector_interval: Duration::from_secs(300),
        };
        let service = NotificationsDeduplicationServiceImpl::new(config);

        let id = ObjectId::new();
        let datetime = OffsetDateTime::now_utc();
        {
            service.last_updates.lock().await.insert(id, datetime);
        }

        let notification = NotificationStatusUpdate {
            id,
            timestamp: datetime - Duration::from_secs(30),
        };

        let result = service.deduplicate(notification).await;

        assert!(matches!(result, Err(Error::Duplicate)));
    }

    #[tokio::test]
    async fn garbage_collector_old_entries_removed() {
        let garbage_collector_interval = Duration::from_millis(500);

        let config = NotificationsDeduplicationServiceConfig {
            notification_lifespan: Duration::from_millis(100),
            garbage_collector_interval,
        };
        let service = NotificationsDeduplicationServiceImpl::new(config);

        let now = OffsetDateTime::now_utc();

        let len_before = {
            let mut map = service.last_updates.lock().await;
            // Two relatively new notifications that should not be removed
            // by garbage collector
            map.insert(ObjectId::new(), now + Duration::from_millis(500));
            map.insert(ObjectId::new(), now + Duration::from_millis(450));
            // Some old notifications that should be removed by garbage collector
            map.insert(ObjectId::new(), now + Duration::from_millis(100));
            map.insert(ObjectId::new(), now + Duration::from_millis(200));

            map.len()
        };

        // wait for garbage collector
        sleep(garbage_collector_interval + Duration::from_millis(10)).await;

        let len_after = service.last_updates.lock().await.len();

        assert!(len_after < len_before);
        assert_eq!(len_after, 2);
    }

    #[tokio::test]
    async fn garbage_collector_entries_not_touched_until_interval_reached() {
        let notification_lifespan = Duration::from_millis(50);
        let garbage_collector_interval = Duration::from_millis(100);
        assert!(notification_lifespan < garbage_collector_interval);

        let config = NotificationsDeduplicationServiceConfig {
            notification_lifespan,
            garbage_collector_interval,
        };
        let service = NotificationsDeduplicationServiceImpl::new(config);

        let len_before = {
            let mut map = service.last_updates.lock().await;
            map.insert(ObjectId::new(), OffsetDateTime::now_utc());
            map.len()
        };

        sleep(notification_lifespan + Duration::from_millis(5)).await;

        let len_after = service.last_updates.lock().await.len();
        assert_eq!(len_after, len_before);
    }
}
