use super::{ApplicationMiddleware, ApplicationState};
use crate::routing::routing;
use axum::Router;
use std::net::SocketAddr;

pub fn create_application(
    application_state: ApplicationState,
    application_middleware: ApplicationMiddleware,
) -> axum::extract::connect_info::IntoMakeServiceWithConnectInfo<Router, SocketAddr> {
    routing(&application_middleware)
        .with_state(application_state)
        .layer(application_middleware.trace)
        .into_make_service_with_connect_info::<SocketAddr>()
}
