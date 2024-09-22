use crate::dto::output;
use axum::async_trait;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait ConfirmationsService: Send + Sync {
    async fn send(&self, confirmation: output::RabbitmqConfirmationProtobuf);
}
