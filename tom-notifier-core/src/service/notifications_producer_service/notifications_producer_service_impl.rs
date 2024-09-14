use super::{NotificationsProducerService, NotificationsProducerServiceConfig};
use crate::dto::output;
use amqprs::{
    channel::{ExchangeDeclareArguments, ExchangeType},
    BasicProperties,
};
use axum::async_trait;
use bson::oid::ObjectId;
use prost::Message;
use prost_types::Timestamp;
use rabbitmq_client::{RabbitmqConnection, RabbitmqProducer};
use time::OffsetDateTime;
use uuid::Uuid;

pub struct NotificationsProducerServiceImpl {
    producer: RabbitmqProducer,
}

impl NotificationsProducerServiceImpl {
    pub async fn new(
        config: NotificationsProducerServiceConfig,
        rabbitmq_connection: RabbitmqConnection,
    ) -> anyhow::Result<Self> {
        let exchange_declare_args =
            ExchangeDeclareArguments::of_type(&config.exchange_name, ExchangeType::Direct);
        let producer = RabbitmqProducer::new(rabbitmq_connection, exchange_declare_args).await?;

        Ok(Self { producer })
    }

    pub async fn close(self) {
        self.producer.close().await;
    }

    fn send(&self, routing_key: &'static str, encoded_message: Vec<u8>) {
        let basic_properties = BasicProperties::default().with_persistence(true).finish();
        self.producer
            .send(routing_key.to_string(), basic_properties, encoded_message);
    }
}

#[async_trait]
impl NotificationsProducerService for NotificationsProducerServiceImpl {
    async fn send_new(
        &self,
        user_ids: Vec<Uuid>,
        id: ObjectId,
        timestamp: OffsetDateTime,
        created_by: Uuid,
        seen: bool,
        content_type: String,
        content: Vec<u8>,
    ) {
        let id_str = id.to_hex();

        tracing::info!(id = id_str, %timestamp, "producing NEW notification");

        let message = output::RabbitmqNotificationProtobuf {
            user_ids: user_ids.into_iter().map(|uuid| uuid.to_string()).collect(),
            notification: Some(output::NotificationProtobuf {
                id: id_str,
                status: output::NotificationStatusProtobuf::New.into(),
                timestamp: Some(Timestamp {
                    seconds: timestamp.unix_timestamp(),
                    nanos: timestamp.nanosecond() as i32,
                }),
                created_by: Some(created_by.to_string()),
                seen: Some(seen),
                content_type: Some(content_type),
                content: Some(content),
            }),
        };
        let encoded_message = message.encode_to_vec();

        self.send("NEW", encoded_message);
    }

    async fn send_updated(
        &self,
        user_id: Uuid,
        id: ObjectId,
        seen: bool,
        timestamp: OffsetDateTime,
    ) {
        let id_str = id.to_hex();

        tracing::info!(id = id_str, %timestamp, "producing UPDATED notification");

        let message = output::RabbitmqNotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            notification: Some(output::NotificationProtobuf {
                id: id_str,
                status: output::NotificationStatusProtobuf::Updated.into(),
                timestamp: Some(Timestamp {
                    seconds: timestamp.unix_timestamp(),
                    nanos: timestamp.nanosecond() as i32,
                }),
                created_by: None,
                seen: Some(seen),
                content_type: None,
                content: None,
            }),
        };
        let encoded_message = message.encode_to_vec();

        self.send("UPDATED", encoded_message);
    }

    async fn send_deleted(&self, user_id: Uuid, id: ObjectId, timestamp: OffsetDateTime) {
        let id_str = id.to_hex();

        tracing::info!(id = id_str, %timestamp, "producing DELETED notification");

        let message = output::RabbitmqNotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            notification: Some(output::NotificationProtobuf {
                id: id_str,
                status: output::NotificationStatusProtobuf::Deleted.into(),
                timestamp: Some(Timestamp {
                    seconds: timestamp.unix_timestamp(),
                    nanos: timestamp.nanosecond() as i32,
                }),
                created_by: None,
                seen: None,
                content_type: None,
                content: None,
            }),
        };
        let encoded_message = message.encode_to_vec();

        self.send("DELETED", encoded_message);
    }
}
