use super::ApplicationState;
use crate::routing::routing;
use axum::Router;

pub fn create_application(application_state: ApplicationState) -> Router {
    routing().with_state(application_state)
}
