use crate::service::websockets_service::websocket_confirmation_callback::WebSocketConfirmationCallback;
use uuid::Uuid;

pub struct WebSocketMessage {
    pub message_id: Uuid,
    pub payload: Vec<u8>,

    ///
    /// Callback executed when user confirms the message
    ///
    pub delivered_callback: Option<WebSocketConfirmationCallback>,
}
