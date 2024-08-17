use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        tracing::warn!(err = %self);

        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}
