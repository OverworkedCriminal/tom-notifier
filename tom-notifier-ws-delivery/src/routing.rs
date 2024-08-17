use crate::{
    application::{ApplicationMiddleware, ApplicationState},
    dto::{input, output},
    error::Error,
};
use axum::{
    extract::{Path, Query, WebSocketUpgrade},
    http::StatusCode,
    response::Response,
    routing::{delete, get},
    Json, Router,
};
use uuid::Uuid;

pub fn routing(application_middleware: &ApplicationMiddleware) -> Router<ApplicationState> {
    Router::new()
        .route("/api/v1/ticket", get(get_ticket))
        .route("/api/v1/connection/:user_id", delete(delete_connection))
        .route_layer(application_middleware.auth.clone())
        .route("/ws/v1", get(websocket_upgrade))
}

async fn get_ticket() -> Result<(StatusCode, Json<output::WebSocketTicket>), Error> {
    todo!("not implemented")
}

async fn delete_connection(Path(user_id): Path<Uuid>) -> Result<StatusCode, Error> {
    todo!("not implemented")
}

async fn websocket_upgrade(
    Query(ticket): Query<input::WebSocketTicket>,
    ws: WebSocketUpgrade,
) -> Result<Response, Error> {
    todo!("not implemented")
}
