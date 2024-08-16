use amqprs::{callbacks::ConnectionCallback, connection::Connection, Close};
use async_trait::async_trait;
use tokio::sync::watch;

#[derive(Clone)]
pub struct RabbitmqConnectionCallback {
    blocked_tx: watch::Sender<bool>,
}

impl RabbitmqConnectionCallback {
    pub fn new(blocked_tx: watch::Sender<bool>) -> Self {
        Self { blocked_tx }
    }
}

#[async_trait]
impl ConnectionCallback for RabbitmqConnectionCallback {
    #[tracing::instrument(
        name = "RabbitMQ Connection Callback",
        target = "rabbitmq_client::connection_callback",
        skip_all
    )]
    async fn close(
        &mut self,
        _connection: &Connection,
        close: Close,
    ) -> Result<(), amqprs::error::Error> {
        tracing::warn!(
            code = close.reply_code(),
            text = close.reply_text(),
            "received close",
        );

        Ok(())
    }

    #[tracing::instrument(
        name = "RabbitMQ Connection Callback",
        target = "rabbitmq_client::connection_callback",
        skip_all
    )]
    async fn blocked(&mut self, _connection: &Connection, reason: String) {
        tracing::warn!(reason, "received blocked");

        self.blocked_tx.send_replace(true);
    }

    #[tracing::instrument(
        name = "RabbitMQ Connection Callback",
        target = "rabbitmq_client::connection_callback",
        skip_all
    )]
    async fn unblocked(&mut self, _connection: &Connection) {
        tracing::info!("received unblocked");

        self.blocked_tx.send_replace(false);
    }
}
