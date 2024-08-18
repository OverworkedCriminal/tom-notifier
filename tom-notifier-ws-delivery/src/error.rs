use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use jwt_auth::error::MissingRoleError;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("auth error: {0}")]
    Auth(#[from] MissingRoleError),
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        tracing::warn!(err = %self);

        match self {
            Error::Auth(_) => StatusCode::FORBIDDEN,
        }
        .into_response()
    }
}
