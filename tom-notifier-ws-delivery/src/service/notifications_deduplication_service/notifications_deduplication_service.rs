use crate::{dto::input, error::Error};
use axum::async_trait;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait NotificationsDeduplicationService: Send + Sync {
    ///
    /// Function takes notification and makes sure it is not a duplicate.
    ///
    /// ### Errors
    /// - [Error::NotificationAlreadyProcessed]
    ///
    async fn deduplicate(&self, notification: &input::NotificationProtobuf) -> Result<(), Error>;
}
