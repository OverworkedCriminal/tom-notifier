use crate::dto::output;
use axum::async_trait;

#[async_trait]
pub trait ConfirmationsService: Send + Sync {
    async fn send(&self, confirmation: output::ConfirmationProtobuf);
}
