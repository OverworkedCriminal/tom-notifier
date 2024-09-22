use super::{ApplicationMiddleware, ApplicationState};
use crate::routing::routing;
use axum::Router;

pub fn create_application(
    application_state: ApplicationState,
    application_middleware: ApplicationMiddleware,
) -> Router {
    routing()
        .with_state(application_state)
        .layer(application_middleware.body_limit)
        .route_layer(application_middleware.auth)
        .layer(application_middleware.trace)
}
