use crate::{
    dto::{input, output},
    error::Error,
};
use axum::async_trait;
use bson::oid::ObjectId;
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait NotificationsService: Send + Sync {
    ///
    /// Save new notification in application.
    ///
    /// ### Returns
    /// ID of created notification
    ///
    /// ### Errors
    /// - [Error::Validation] when
    ///     - invalidate_at already passed
    /// - [Error::ValidationNotificationTooLarge] when
    ///     - notification content is too long
    /// - [Error::NotificationAlreadySaved] when producer
    ///    already created notification with producer_notification_id
    ///
    async fn save_notification(
        &self,
        producer_id: Uuid,
        notification: input::Notification,
    ) -> Result<output::NotificationId, Error>;

    ///
    /// Find all undelivered notifications that belong to the user
    /// and mark them as delivered.
    ///
    /// ### Returns
    /// Vec of undelivered notifications
    ///
    async fn find_undelivered_notifications(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<output::Notification>, Error>;

    ///
    /// Find all delivered notifications that belong to the user
    /// and match filters
    ///
    /// ### Returns
    /// Vec of delivered notifications
    ///
    async fn find_delivered_notifications(
        &self,
        user_id: Uuid,
        pagination: input::Pagination,
        filters: input::NotificationFilters,
    ) -> Result<Vec<output::Notification>, Error>;

    ///
    /// Find delivered notification
    ///
    /// ### Returns
    /// notification
    ///
    /// ### Errors
    /// - [Error::NotificationNotExist] when
    ///     - notification with id does not exist
    ///     - user does not belong to notification recipients
    ///     - notification have not been delivered yet
    ///     - notification have already been deleted
    ///
    async fn find_delivered_notification(
        &self,
        id: ObjectId,
        user_id: Uuid,
    ) -> Result<output::Notification, Error>;

    ///
    /// Delete notification
    ///
    /// ### Errors
    /// - [Error::NotificationNotExist] when
    ///     - notification with id does not exist
    ///     - user does not belong to notification recipients
    ///     - notification have not been delivered yet
    ///     - notification have already been deleted
    ///
    async fn delete_notification(&self, id: ObjectId, user_id: Uuid) -> Result<(), Error>;

    ///
    /// Update field invalidate_at of the notification
    ///
    /// ### Errors
    /// - [Error::Validation] when
    ///     - invalidate_at already passed
    /// - [Error::NotificationNotExist] when
    ///     - notification with id does not exist
    ///     - notification was not produced by the producer
    ///
    async fn update_notification_invalidate_at(
        &self,
        id: ObjectId,
        producer_id: Uuid,
        invalidate_at: input::NotificationInvalidateAt,
    ) -> Result<(), Error>;

    ///
    /// Update field seen of the notification
    ///
    /// ### Errors
    /// - [Error::NotificationNotExist] when
    ///     - notification with id does not exist
    ///     - user does not belong to notification recipients
    ///     - notification have not been delivered yet
    ///     - notification is deleted
    ///
    async fn update_notification_seen(
        &self,
        id: ObjectId,
        user_id: Uuid,
        seen: input::NotificationSeen,
    ) -> Result<(), Error>;
}
