use super::NotificationsConsumerServiceConfig;
use crate::{
    dto::{input, output},
    service::websockets_service::WebSocketsService,
};
use amqprs::{
    channel::{
        BasicAckArguments, BasicConsumeArguments, BasicNackArguments, Channel,
        ExchangeDeclareArguments, ExchangeType, QueueBindArguments, QueueDeclareArguments,
    },
    consumer::AsyncConsumer,
    BasicProperties, Deliver,
};
use anyhow::anyhow;
use axum::async_trait;
use prost::Message;
use rabbitmq_client::{
    RabbitmqConnection, RabbitmqConsumer, RabbitmqConsumerStatus,
    RabbitmqConsumerStatusChangeCallback,
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
        let consumer = Consumer {
            websockets_service: Arc::clone(&websockets_service),
        };
        let status_callback = StatusCallback { websockets_service };
        let rabbitmq_consumer = RabbitmqConsumer::new(
            rabbitmq_connection,
            exchange_declare_args,
            queue_declare_args,
            queue_bind_args,
            basic_consume_args,
            consumer,
            status_callback,
        )
        .await?;

        Ok(Self { rabbitmq_consumer })
    }

    pub async fn close(self) {
        self.rabbitmq_consumer.close().await;
    }
}

#[derive(Clone)]
struct Consumer {
    websockets_service: Arc<dyn WebSocketsService>,
}

impl Consumer {
    async fn try_consume(&self, content: Vec<u8>) -> anyhow::Result<()> {
        let message = input::RabbitmqNotificationProtobuf::decode(content.as_slice())
            .map_err(|err| anyhow!("invalid notification: {err}"))?;

        let mut user_ids = Vec::with_capacity(message.user_ids.len());
        for uuid_str in message.user_ids {
            let uuid =
                Uuid::from_str(&uuid_str).map_err(|err| anyhow!("invalid user id: {err}"))?;
            user_ids.push(uuid);
        }

        let Some(notification) = message.notification else {
            anyhow::bail!("invalid notification: notification cannot be null");
        };

        self.websockets_service.send(&user_ids, notification).await;

        Ok(())
    }
}

#[async_trait]
impl AsyncConsumer for Consumer {
    #[tracing::instrument(
        name = "Notifications Consumer",
        skip_all,
        fields(
            delivery_tag = deliver.delivery_tag(),
        )
    )]
    async fn consume(
        &mut self,
        channel: &Channel,
        deliver: Deliver,
        _basic_properties: BasicProperties,
        content: Vec<u8>,
    ) {
        tracing::info!("processing notification");

        match self.try_consume(content).await {
            Ok(()) => {
                tracing::trace!("sending ack");
                let args = BasicAckArguments::new(deliver.delivery_tag(), false);
                match channel.basic_ack(args).await {
                    Ok(()) => tracing::trace!("ack sent"),
                    Err(err) => tracing::warn!(%err, "failed to ack message"),
                }
            }
            Err(err) => {
                tracing::warn!(%err, "failed to consume notification");
                tracing::trace!("sending nack");
                let args = BasicNackArguments::new(deliver.delivery_tag(), false, false);
                match channel.basic_nack(args).await {
                    Ok(()) => tracing::trace!("nack sent"),
                    Err(err) => tracing::warn!(%err, "failed to nack message"),
                }
            }
        }

        tracing::info!("notification processed");
    }
}

struct StatusCallback {
    websockets_service: Arc<dyn WebSocketsService>,
}
#[async_trait]
impl RabbitmqConsumerStatusChangeCallback for StatusCallback {
    async fn execute(&self, status: RabbitmqConsumerStatus) {
        let network_status = match status {
            RabbitmqConsumerStatus::Consuming => output::NetworkStatusProtobuf::Ok,
            RabbitmqConsumerStatus::Recovering => output::NetworkStatusProtobuf::Error,
        };

        self.websockets_service
            .update_network_status(network_status)
            .await;
    }
}
