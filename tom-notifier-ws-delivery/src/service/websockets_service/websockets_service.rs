use crate::dto::output;
use axum::{async_trait, extract::ws::WebSocket};
use std::net::SocketAddr;
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait WebSocketsService: Send + Sync {
    async fn handle_client(&self, user_id: Uuid, address: SocketAddr, websocket: WebSocket);

    async fn close_connections(&self, user_id: Uuid);

    async fn send(&self, user_ids: &[Uuid], notification: output::NotificationProtobuf);

    async fn update_network_status(&self, status: output::NetworkStatusProtobuf);
}
