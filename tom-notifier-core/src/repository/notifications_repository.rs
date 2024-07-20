use super::{dto::Notification, error::Error};
use crate::dto::input;
use axum::async_trait;
use bson::oid::ObjectId;
use time::OffsetDateTime;
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait NotificationsRepository: Send + Sync {
    ///
    /// Inserts new notification.
    /// If user_ids is empty, inserts broadcast notification
    ///
    /// ### Errors
    /// - [Error::InsertUniqueViolation]
    /// when pair (producer_id, producer_notification_id) is not unique
    ///
    async fn insert(
        &self,
        user_ids: &[Uuid],
        created_at: OffsetDateTime,
        invalidate_at: Option<OffsetDateTime>,
        producer_id: Uuid,
        producer_notification_id: i64,
        content_type: &str,
        content: &[u8],
    ) -> Result<ObjectId, Error>;

    ///
    /// Updates notification invalidate_at
    ///
    /// ### Errors
    /// - [Error::NoDocumentUpdated] when
    ///     - notification does not exist
    ///     - notification was not produced by producer
    ///
    async fn update_invalidate_at(
        &self,
        id: ObjectId,
        producer_id: Uuid,
        invalidate_at: Option<OffsetDateTime>,
    ) -> Result<(), Error>;

    ///
    /// Inserts new confirmation for the notification.
    ///
    /// ### Errors
    /// - Returns [Error::NoDocumentUpdated] when
    ///     - notification already has user's confirmation
    ///     - notification does not belong to the user and it's not a broadcast notification
    ///     - notification was invalidated
    ///
    async fn insert_confirmation(&self, id: ObjectId, user_id: Uuid) -> Result<(), Error>;

    ///
    /// Insert new confirmation for each of the notifications
    ///
    async fn insert_many_confirmations(&self, ids: &[ObjectId], user_id: Uuid)
        -> Result<(), Error>;

    ///
    /// Updates confirmation seen value.
    ///
    /// ### Errors
    /// - [Error::NoDocumentUpdated] when
    ///     - notification does not exist
    ///     - notification does not belong to the user and it's not a brodcast notification
    ///     - user didn't confirm receiving notification
    ///     - notification has property deleted = true
    ///
    async fn update_confirmation_seen(
        &self,
        id: ObjectId,
        user_id: Uuid,
        seen: bool,
    ) -> Result<(), Error>;

    ///
    /// Marks notification as deleted
    ///
    /// ### Errors
    /// - [Error::NoDocumentUpdated] when
    ///     - notification does not exist
    ///     - notification does not belong to the user
    ///     - user didn't confirm receiving notification
    ///     - notification has property deleted = true
    ///
    async fn delete(&self, id: ObjectId, user_id: Uuid) -> Result<(), Error>;

    ///
    /// Finds one delivered notification notification
    ///
    async fn find_delivered(
        &self,
        id: ObjectId,
        user_id: Uuid,
    ) -> Result<Option<Notification>, Error>;

    ///
    /// Finds notifications that were already delivered to the user.
    /// Notifications are sorted descending by creation date.
    ///
    async fn find_many_delivered(
        &self,
        user_id: Uuid,
        pagination: input::Pagination,
        filters: input::NotificationFilters,
    ) -> Result<Vec<Notification>, Error>;

    ///
    /// Finds all notifications that were not received by the user.
    /// Notifications are sorted ascending by creation date.
    ///
    async fn find_many_undelivered(&self, user_id: Uuid) -> Result<Vec<Notification>, Error>;
}
