use super::RabbitmqConsumerStatus;
use async_trait::async_trait;

///
/// Callback executed whenever status of the consumer changes
/// 
#[async_trait]
pub trait RabbitmqConsumerStatusChangeCallback {
    async fn execute(&self, status: RabbitmqConsumerStatus);
}
