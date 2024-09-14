use super::{dto::Confirmation, ConfirmationsConsumerServiceConfig};
use crate::{
    dto::input,
    repository::{self, NotificationsRepository},
};
use amqprs::channel::{
    BasicConsumeArguments, ExchangeDeclareArguments, ExchangeType, QueueBindArguments,
    QueueDeclareArguments,
};
use axum::async_trait;
use prost::Message;
use rabbitmq_client::{
    connection::RabbitmqConnection,
    consumer::{
        callback::{RabbitmqConsumerDeliveryCallback, RabbitmqConsumerStatusChangeCallback},
        error::ConsumeError,
        RabbitmqConsumer, RabbitmqConsumerStatus,
    },
};
use std::sync::Arc;

pub struct ConfirmationsConsumerService {
    rabbitmq_consumer: RabbitmqConsumer,
}

impl ConfirmationsConsumerService {
    pub async fn new(
        config: ConfirmationsConsumerServiceConfig,
        rabbitmq_connection: RabbitmqConnection,
        notifications_repository: Arc<dyn NotificationsRepository>,
    ) -> anyhow::Result<Self> {
        let exchange_declare_args =
            ExchangeDeclareArguments::of_type(&config.exchange, ExchangeType::Direct)
                .durable(true)
                .finish();
        let queue_declare_args = QueueDeclareArguments::new(&config.queue)
            .durable(true)
            .finish();
        let queue_bind_args = QueueBindArguments::new(&config.queue, &config.exchange, "");
        let basic_consume_args = BasicConsumeArguments::new(&config.queue, "")
            .auto_ack(false)
            .finish();
        let delivery_callback = DeliveryCallback {
            notifications_repository,
        };
        let status_callback = StatusCallback;
        let rabbitmq_consumer = RabbitmqConsumer::new(
            rabbitmq_connection,
            exchange_declare_args,
            queue_declare_args,
            vec![queue_bind_args],
            basic_consume_args,
            delivery_callback,
            status_callback,
        )
        .await?;

        Ok(Self { rabbitmq_consumer })
    }

    pub async fn close(self) {
        self.rabbitmq_consumer.close().await;
    }
}

struct DeliveryCallback {
    notifications_repository: Arc<dyn NotificationsRepository>,
}

#[async_trait]
impl RabbitmqConsumerDeliveryCallback for DeliveryCallback {
    async fn execute(&self, content: Vec<u8>) -> Result<(), ConsumeError> {
        let message =
            input::RabbitmqConfirmationProtobuf::decode(content.as_slice()).map_err(|err| {
                tracing::warn!(%err, "invalid confirmation");
                ConsumeError { requeue: false }
            })?;

        let id_str = message.id.clone();
        let user_id_str = message.user_id.clone();

        let confirmation = Confirmation::try_from(message).map_err(|err| {
            tracing::warn!(%err, "invalid confirmation");
            ConsumeError { requeue: false }
        })?;

        tracing::info!(id = id_str, user_id = user_id_str, "inserting confirmation");
        match self
            .notifications_repository
            .insert_confirmation(confirmation.id, confirmation.user_id)
            .await
        {
            Ok(()) => {
                tracing::info!("confirmation inserted");
                Ok(())
            }
            Err(repository::Error::NoDocumentUpdated) => {
                tracing::info!("confirmation already exist");
                Ok(())
            }
            Err(err) => {
                tracing::warn!(%err, "failed to insert confirmation");
                Err(ConsumeError { requeue: true })
            }
        }
    }
}

struct StatusCallback;
#[async_trait]
impl RabbitmqConsumerStatusChangeCallback for StatusCallback {
    async fn execute(&self, _status: RabbitmqConsumerStatus) {}
}
