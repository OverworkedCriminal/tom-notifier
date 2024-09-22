use super::NotificationStatusUpdate;
use crate::error::Error;
use axum::async_trait;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait NotificationsDeduplicationService: Send + Sync {
    ///
    /// Function takes notification and makes sure it is not a duplicate.
    ///
    /// ### Errors
    /// - [Error::Duplicate]
    ///
    async fn deduplicate(&self, notification: NotificationStatusUpdate) -> Result<(), Error>;
}
