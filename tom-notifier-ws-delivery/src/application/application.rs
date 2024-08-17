use super::{ApplicationMiddleware, ApplicationState};
use crate::routing::routing;
use axum::Router;

pub fn create_application(
    application_state: ApplicationState,
    application_middleware: ApplicationMiddleware,
) -> Router {
    routing(&application_middleware)
        .with_state(application_state)
        .layer(application_middleware.trace)
}
