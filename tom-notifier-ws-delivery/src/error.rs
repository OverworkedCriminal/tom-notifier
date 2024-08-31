use crate::repository;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use jwt_auth::error::MissingRoleError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("auth error: {0}")]
    Auth(#[from] MissingRoleError),

    #[error("database error: {0}")]
    Database(#[from] repository::Error),

    #[error("websocket ticket error: {0}")]
    TicketInvalid(&'static str),

    #[error("notification already processed")]
    NotificationAlreadyProcessed,

    ///
    /// This error should be returned only in situations
    /// that should never occur when system is setup correctly.
    ///
    #[error("unexpected error: {0}")]
    UnexpectedError(#[from] anyhow::Error),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        tracing::warn!(err = %self);

        match self {
            Error::Auth(_) => StatusCode::FORBIDDEN,
            Error::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::TicketInvalid(_) => StatusCode::UNAUTHORIZED,
            Error::NotificationAlreadyProcessed => StatusCode::CONFLICT,
            Error::UnexpectedError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
        .into_response()
    }
}
