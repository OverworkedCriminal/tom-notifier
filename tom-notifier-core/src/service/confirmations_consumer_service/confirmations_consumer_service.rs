use super::{dto::Confirmation, ConfirmationsConsumerServiceConfig};
use crate::{
    dto::input,
    repository::{self, NotificationsRepository},
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
use rabbitmq_client::{RabbitmqConnection, RabbitmqConsumer};
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
        let consumer = Consumer {
            notifications_repository,
        };
        let rabbitmq_consumer = RabbitmqConsumer::new(
            rabbitmq_connection,
            exchange_declare_args,
            queue_declare_args,
            vec![queue_bind_args],
            basic_consume_args,
            consumer,
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
    notifications_repository: Arc<dyn NotificationsRepository>,
}

impl Consumer {
    async fn try_consume(&self, content: Vec<u8>) -> anyhow::Result<()> {
        let message = input::ConfirmationProtobuf::decode(content.as_slice())
            .map_err(|err| anyhow!("invalid confirmation: {err}"))?;

        let id_str = message.id.clone();
        let user_id_str = message.user_id.clone();

        let confirmation = Confirmation::try_from(message)
            .map_err(|err| anyhow!("invalid confirmation: {err}"))?;

        tracing::info!(id = id_str, user_id = user_id_str, "inserting confirmation");
        match self
            .notifications_repository
            .insert_confirmation(confirmation.id, confirmation.user_id)
            .await
        {
            Ok(()) => tracing::info!("confirmation inserted"),
            Err(repository::Error::NoDocumentUpdated) => {
                tracing::info!("confirmation already exist")
            }
            Err(err) => anyhow::bail!("failed to insert confirmation: {err}"),
        }

        Ok(())
    }
}

#[async_trait]
impl AsyncConsumer for Consumer {
    #[tracing::instrument(
        name = "Confirmations Consumer",
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
        tracing::info!("processing confirmation");

        match self.try_consume(content).await {
            Ok(()) => {
                tracing::trace!("sending ack");
                let args = BasicAckArguments::new(deliver.delivery_tag(), false);
                if let Err(err) = channel.basic_ack(args).await {
                    tracing::warn!(%err, "failed to ack message")
                }
                tracing::trace!("ack sent");
            }
            Err(err) => {
                tracing::warn!(%err, "failed to consume confirmation");
                tracing::trace!("sending nack");
                let args = BasicNackArguments::new(deliver.delivery_tag(), false, true);
                if let Err(err) = channel.basic_nack(args).await {
                    tracing::warn!(%err, "failed to nack message");
                }
                tracing::trace!("nack sent");
            }
        }

        tracing::info!("confirmation processed");
    }
}
