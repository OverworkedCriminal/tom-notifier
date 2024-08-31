use super::NotificationsDeduplicationService;
use crate::{dto::input, error::Error};
use anyhow::anyhow;
use axum::async_trait;
use bson::oid::ObjectId;
use std::{collections::HashMap, str::FromStr};
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
    /// Function takes notification and makes sure it is not a duplicate.
    ///
    /// ### Returns
    /// - true when notification IS NOT a duplicate
    /// - false when notification IS a duplicate
    ///
    /// ### Errors
    /// - [Error::NotificationAlreadyProcessed]
    /// - [Error::UnexpectedError] when notification is not valid
    ///
    async fn deduplicate(&self, notification: &input::NotificationProtobuf) -> Result<(), Error> {
        let id = ObjectId::from_str(&notification.id)
            .map_err(|err| anyhow!("notification invalid: invalid object id: {err}"))?;
        let timestamp = notification
            .timestamp
            .ok_or_else(|| anyhow!("notification invalid: missing timestamp field"))?;
        let new_datetime = OffsetDateTime::from_unix_timestamp(timestamp.seconds)
            .map_err(|err| {
                anyhow!("notification invalid: invalid timestamp: invalid seconds: {err}")
            })?
            .replace_nanosecond(timestamp.nanos as u32)
            .map_err(|err| {
                anyhow!("notification invalid: invalid timestamp: invalid nanos: {err}")
            })?;

        let mut last_updates = self.last_updates.lock().await;

        match last_updates.get_mut(&id) {
            Some(datetime) if *datetime < new_datetime => {
                *datetime = new_datetime;
                Ok(())
            }
            Some(_) => Err(Error::NotificationAlreadyProcessed),
            None => {
                last_updates.insert(id, new_datetime);
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use prost_types::Timestamp;
    use std::time::Duration;
    use uuid::Uuid;

    #[tokio::test]
    async fn deduplicate_first_entry() {
        let service = NotificationsDeduplicationServiceImpl::new();

        let now = OffsetDateTime::now_utc();
        let notification = input::NotificationProtobuf {
            id: ObjectId::new().to_hex(),
            status: input::NotificationStatusProtobuf::New.into(),
            timestamp: Some(Timestamp {
                seconds: now.unix_timestamp(),
                nanos: now.nanosecond() as i32,
            }),
            created_by: Some(Uuid::new_v4().to_string()),
            seen: Some(false),
            content_type: Some("utf-8".to_string()),
            content: Some(b"text".to_vec()),
        };

        let result = service.deduplicate(&notification).await;

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

        let new_datetime = datetime + Duration::from_secs(30);
        let notification = input::NotificationProtobuf {
            id: id.to_hex(),
            status: input::NotificationStatusProtobuf::New.into(),
            timestamp: Some(Timestamp {
                seconds: new_datetime.unix_timestamp(),
                nanos: new_datetime.nanosecond() as i32,
            }),
            created_by: Some(Uuid::new_v4().to_string()),
            seen: Some(false),
            content_type: Some("utf-8".to_string()),
            content: Some(b"text".to_vec()),
        };

        let result = service.deduplicate(&notification).await;

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

        let new_datetime = datetime - Duration::from_secs(30);
        let notification = input::NotificationProtobuf {
            id: id.to_hex(),
            status: input::NotificationStatusProtobuf::New.into(),
            timestamp: Some(Timestamp {
                seconds: new_datetime.unix_timestamp(),
                nanos: new_datetime.nanosecond() as i32,
            }),
            created_by: Some(Uuid::new_v4().to_string()),
            seen: Some(false),
            content_type: Some("utf-8".to_string()),
            content: Some(b"text".to_vec()),
        };

        let result = service.deduplicate(&notification).await;

        assert!(matches!(result, Err(Error::NotificationAlreadyProcessed)));
    }
}
