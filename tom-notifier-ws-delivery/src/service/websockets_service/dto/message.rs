use std::{future::Future, pin::Pin};
use uuid::Uuid;

pub struct Message {
    pub message_id: Uuid,
    pub payload: Vec<u8>,

    ///
    /// Callback called when user confirms the message.
    /// Argument passed to callback is 'user_id'.
    ///
    pub delivered_callback:
        Option<Box<dyn FnOnce(Uuid) -> Pin<Box<dyn Future<Output = ()>>> + Send + Sync>>,
}
