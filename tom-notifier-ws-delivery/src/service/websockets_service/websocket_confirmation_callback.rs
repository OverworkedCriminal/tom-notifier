use crate::{dto::output, service::confirmations_service::ConfirmationsService};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use uuid::Uuid;

pub struct WebSocketConfirmationCallback {
    confirmations_service: Arc<dyn ConfirmationsService>,
    was_called: AtomicBool,

    id: String,
}

impl WebSocketConfirmationCallback {
    pub fn new(confirmations_service: Arc<dyn ConfirmationsService>, id: String) -> Self {
        Self {
            confirmations_service,
            was_called: AtomicBool::new(false),
            id,
        }
    }

    pub async fn execute(&self, user_id: Uuid) {
        let was_called = self.was_called.swap(true, Ordering::AcqRel);
        if was_called {
            // There's no point in sending multiple confirmations
            // of the same message
            return;
        }

        let confirmation = output::RabbitmqConfirmationProtobuf {
            user_id: user_id.to_string(),
            id: self.id.clone(),
        };

        tracing::info!("sending confirmation");
        self.confirmations_service.send(confirmation).await;
    }
}
