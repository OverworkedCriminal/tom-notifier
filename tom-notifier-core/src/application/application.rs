use super::ApplicationState;
use axum::Router;

pub fn create_application(application_state: ApplicationState) -> Router {
    Router::new().with_state(application_state)
}
