use super::NotificationsConsumerServiceConfig;
use crate::{
    dto::{input, output},
    error::Error,
    service::{
        notifications_deduplication_service::{
            NotificationStatusUpdate, NotificationsDeduplicationService,
        },
        websockets_service::WebSocketsService,
    },
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
use std::{str::FromStr, sync::Arc};
use uuid::Uuid;

pub struct NotificationsConsumerService {
    rabbitmq_consumer: RabbitmqConsumer,
}

impl NotificationsConsumerService {
    pub async fn new(
        config: NotificationsConsumerServiceConfig,
        rabbitmq_connection: RabbitmqConnection,
        websockets_service: Arc<dyn WebSocketsService>,
        deduplication_service: Arc<dyn NotificationsDeduplicationService>,
    ) -> anyhow::Result<Self> {
        let queue = format!("{}_{}", config.queue, Uuid::new_v4());
        let exchange_declare_args =
            ExchangeDeclareArguments::of_type(&config.exchange, ExchangeType::Direct);
        let queue_declare_args = QueueDeclareArguments::new(&queue)
            .durable(false)
            .exclusive(true)
            .auto_delete(true)
            .finish();
        let queue_bind_args = vec![
            QueueBindArguments::new(&queue, &config.exchange, "NEW"),
            QueueBindArguments::new(&queue, &config.exchange, "UPDATED"),
            QueueBindArguments::new(&queue, &config.exchange, "DELETED"),
        ];
        let basic_consume_args = BasicConsumeArguments::new(&queue, "")
            .manual_ack(true)
            .exclusive(true)
            .finish();
        let delivery_callback = DeliveryCallback {
            websockets_service: Arc::clone(&websockets_service),
            deduplication_service,
        };
        let status_callback = StatusCallback { websockets_service };
        let rabbitmq_consumer = RabbitmqConsumer::new(
            rabbitmq_connection,
            exchange_declare_args,
            queue_declare_args,
            queue_bind_args,
            basic_consume_args,
            delivery_callback,
            status_callback,
        )
        .await?;

        Ok(Self { rabbitmq_consumer })
    }

    pub async fn close(self) {
        tracing::info!("closing notifications consumer");

        self.rabbitmq_consumer.close().await;

        tracing::info!("notifications consumer closed");
    }
}

struct DeliveryCallback {
    websockets_service: Arc<dyn WebSocketsService>,
    deduplication_service: Arc<dyn NotificationsDeduplicationService>,
}

#[async_trait]
impl RabbitmqConsumerDeliveryCallback for DeliveryCallback {
    async fn execute(&self, content: Vec<u8>) -> Result<(), ConsumeError> {
        let message =
            input::RabbitmqNotificationProtobuf::decode(content.as_slice()).map_err(|err| {
                tracing::warn!(%err, "invalid notification");
                ConsumeError { requeue: false }
            })?;

        let notification = message.notification.ok_or_else(|| {
            tracing::warn!("invalid notification: notification cannot be null");
            ConsumeError { requeue: false }
        })?;

        let mut user_ids = Vec::with_capacity(message.user_ids.len());
        for uuid_str in message.user_ids {
            let uuid = Uuid::from_str(&uuid_str).map_err(|err| {
                tracing::warn!(%err, "invalid user_id");
                ConsumeError { requeue: false }
            })?;
            user_ids.push(uuid);
        }

        let notification_status_update = NotificationStatusUpdate::try_from(&notification)
            .map_err(|err| {
                tracing::warn!(%err, "notification invalid");
                ConsumeError { requeue: false }
            })?;

        tracing::trace!("deduplicating notification");
        match self
            .deduplication_service
            .deduplicate(notification_status_update)
            .await
        {
            Ok(()) => {
                tracing::info!(id = notification.id, "sending notification to clients");
                self.websockets_service.send(&user_ids, notification).await;
                Ok(())
            }
            Err(Error::Duplicate) => {
                tracing::trace!("notification already processed");
                Ok(())
            }
            Err(err) => {
                tracing::warn!(%err, "failed to deduplicate notification");
                Err(ConsumeError {
                    requeue: matches!(err, Error::Database(_)),
                })
            }
        }
    }
}

struct StatusCallback {
    websockets_service: Arc<dyn WebSocketsService>,
}
#[async_trait]
impl RabbitmqConsumerStatusChangeCallback for StatusCallback {
    async fn execute(&self, status: RabbitmqConsumerStatus) {
        tracing::info!(?status, "processing consumer status change");

        let network_status = match status {
            RabbitmqConsumerStatus::Consuming => output::NetworkStatusProtobuf::Ok,
            RabbitmqConsumerStatus::Recovering => output::NetworkStatusProtobuf::Error,
        };

        tracing::info!(?network_status, "sending network status update to clients");
        self.websockets_service
            .update_network_status(network_status)
            .await;

        tracing::info!(?status, "consumer status change processed");
    }
}
