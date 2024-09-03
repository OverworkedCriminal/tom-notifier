use super::{NotificationStatusUpdate, NotificationsDeduplicationService};
use crate::error::Error;
use axum::async_trait;
use bson::oid::ObjectId;
use std::collections::HashMap;
use time::OffsetDateTime;
use tokio::sync::Mutex;

pub struct NotificationsDeduplicationServiceImpl {
    last_updates: Mutex<HashMap<ObjectId, OffsetDateTime>>,
}

impl NotificationsDeduplicationServiceImpl {
    pub fn new() -> Self {
        let last_updates = HashMap::new();
        let last_updates = Mutex::new(last_updates);

        Self { last_updates }
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

    #[tokio::test]
    async fn deduplicate_first_entry() {
        let service = NotificationsDeduplicationServiceImpl::new();

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
        let service = NotificationsDeduplicationServiceImpl::new();

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
        let service = NotificationsDeduplicationServiceImpl::new();

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
}
