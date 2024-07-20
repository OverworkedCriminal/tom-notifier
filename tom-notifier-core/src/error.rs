#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("notification not exist")]
    NotificationNotExist,

    #[error("validation error: {0}")]
    Validation(&'static str),

    #[error("notification already saved")]
    NotificationAlreadySaved,
}
