use axum::{body::Body, http::StatusCode, response::Response};
use pin_project::pin_project;
use std::{
    future::Future,
    pin::Pin,
    task::{ready, Context, Poll},
};
use tracing::Span;

#[pin_project(project = JwtAuthFutureProj)]
pub enum JwtAuthFuture<F> {
    Authorized {
        #[pin]
        inner: F,

        /// span that should be used to add
        /// user context to request processing
        span: Span,
    },
    Unauthorized,
}

impl<F, E> Future for JwtAuthFuture<F>
where
    F: Future<Output = Result<Response, E>>,
{
    type Output = Result<Response, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let result = match self.project() {
            JwtAuthFutureProj::Authorized { inner, span } => {
                let _entered = span.enter();
                ready!(inner.poll(cx))
            }
            JwtAuthFutureProj::Unauthorized => {
                // Since there are no variables here
                // calling unwrap is safe
                let response = unsafe {
                    Response::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::empty())
                        .unwrap_unchecked()
                };
                Ok(response)
            }
        };

        Poll::Ready(result)
    }
}
