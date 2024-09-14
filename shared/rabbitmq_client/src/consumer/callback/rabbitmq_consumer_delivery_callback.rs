use crate::consumer::error::ConsumeError;
use async_trait::async_trait;

///
/// Callback executed whenever delivery is received.
/// 
/// Each delivery is processed in a separate tokio task.
/// Result of this task is passed to 'response task'
/// in order to send Ack or Nack to RabbitMQ server.
///
#[async_trait]
pub trait RabbitmqConsumerDeliveryCallback {
    async fn execute(&self, content: Vec<u8>) -> Result<(), ConsumeError>;
}
