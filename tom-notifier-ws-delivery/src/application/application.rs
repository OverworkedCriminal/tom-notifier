use super::{ApplicationMiddleware, ApplicationState};
use axum::Router;

pub fn create_application(
    application_state: ApplicationState,
    application_middleware: ApplicationMiddleware,
) -> Router {
    Router::new().with_state(application_state)
}
