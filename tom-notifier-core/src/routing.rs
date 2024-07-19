use crate::application::ApplicationState;
use axum::Router;

pub fn routing() -> Router<ApplicationState> {
    Router::new()
}
