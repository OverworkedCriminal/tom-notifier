use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct WebSocketTicket {
    pub ticket: String,
}
