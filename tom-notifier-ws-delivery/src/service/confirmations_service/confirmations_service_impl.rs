use super::{ConfirmationsService, ConfirmationsServiceConfig};
use crate::dto::output;
use amqprs::{
    channel::{ExchangeDeclareArguments, ExchangeType},
    BasicProperties,
};
use axum::async_trait;
use prost::Message;
use rabbitmq_client::{connection::RabbitmqConnection, producer::RabbitmqProducer};

pub struct ConfirmationsServiceImpl {
    producer: RabbitmqProducer,
}

impl ConfirmationsServiceImpl {
    pub async fn new(
        config: ConfirmationsServiceConfig,
        rabbitmq_connection: RabbitmqConnection,
    ) -> anyhow::Result<Self> {
        let exchange_declare_args =
            ExchangeDeclareArguments::of_type(&config.exchange, ExchangeType::Direct)
                .durable(true)
                .finish();
        let producer = RabbitmqProducer::new(rabbitmq_connection, exchange_declare_args).await?;

        Ok(Self { producer })
    }

    pub async fn close(self) {
        tracing::info!("closing confirmations producer");

        self.producer.close().await;

        tracing::info!("confirmations producer closed");
    }
}

#[async_trait]
impl ConfirmationsService for ConfirmationsServiceImpl {
    async fn send(&self, confirmation: output::RabbitmqConfirmationProtobuf) {
        let routing_key = String::new();
        let basic_properties = BasicProperties::default().with_persistence(true).finish();
        let encoded_message = confirmation.encode_to_vec();

        self.producer
            .send(routing_key, basic_properties, encoded_message);

        tracing::info!(
            id = confirmation.id,
            user_id = confirmation.user_id,
            "produced confirmation",
        );
    }
}
