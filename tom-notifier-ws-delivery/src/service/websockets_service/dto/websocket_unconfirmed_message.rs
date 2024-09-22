use super::WebSocketMessage;
use std::sync::Arc;
use tokio::time::Instant;

pub struct WebSocketUnconfirmedMessage {
    pub retry_at: Instant,
    pub retries_remaining: u8,

    pub message: Arc<WebSocketMessage>,
}
