use crate::repository;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use jwt_auth::error::MissingRoleError;

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

    #[error("auth error: {0}")]
    Auth(#[from] MissingRoleError),

    #[error("database error: {0}")]
    Database(#[from] repository::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        tracing::warn!(err = %self);

        match self {
            Error::NotificationNotExist => StatusCode::NOT_FOUND,
            Error::Validation(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Error::ValidationNotificationTooLarge {
                size: _,
                max_size: _,
            } => StatusCode::PAYLOAD_TOO_LARGE,
            Error::NotificationAlreadySaved => StatusCode::CONFLICT,
            Error::Auth(_) => StatusCode::FORBIDDEN,
            Error::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
        .into_response()
    }
}
