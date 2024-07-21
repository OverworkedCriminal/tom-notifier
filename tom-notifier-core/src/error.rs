use crate::repository;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("notification not exist")]
    NotificationNotExist,

    #[error("validation error: {0}")]
    Validation(&'static str),

    #[error("notification already saved")]
    NotificationAlreadySaved,

    #[error("validation error: notification too large {size}/{max_size}B")]
    ValidationNotificationTooLarge { size: usize, max_size: usize },

    #[error("missing role")]
    MissingRole,

    #[error("database error: {0}")]
    Database(#[from] repository::Error),
}
