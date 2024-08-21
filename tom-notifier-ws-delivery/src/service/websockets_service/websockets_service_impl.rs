use super::WebSocketsService;
use crate::dto::output;
use axum::{async_trait, extract::ws::WebSocket};
use std::net::SocketAddr;
use uuid::Uuid;

pub struct WebSocketsServiceImpl {}

impl WebSocketsServiceImpl {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl WebSocketsService for WebSocketsServiceImpl {
    async fn handle_client(&self, user_id: Uuid, address: SocketAddr, websocket: WebSocket) {}

    async fn close_connections(&self, user_id: Uuid) {}

    async fn send(&self, user_ids: &[Uuid], message: output::NotificationProtobuf) {}
}
