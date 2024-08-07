use super::{FanoutService, RabbitmqFanoutServiceConfig};
use crate::dto::protobuf::notification::{NotificationProtobuf, NotificationStatusProtobuf};
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

pub struct RabbitmqFanoutService {
    producer: RabbitmqProducer,
}

impl RabbitmqFanoutService {
    pub async fn new(
        config: RabbitmqFanoutServiceConfig,
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
impl FanoutService for RabbitmqFanoutService {
    async fn send_new(
        &self,
        user_ids: Vec<Uuid>,
        id: ObjectId,
        timestamp: OffsetDateTime,
        seen: bool,
        content_type: String,
        content: Vec<u8>,
    ) {
        let message = NotificationProtobuf {
            user_ids: user_ids.into_iter().map(|uuid| uuid.to_string()).collect(),
            id: id.to_hex(),
            status: NotificationStatusProtobuf::New.into(),
            timestamp: Some(Timestamp {
                seconds: timestamp.unix_timestamp(),
                nanos: timestamp.nanosecond() as i32,
            }),
            seen: Some(seen),
            content_type: Some(content_type),
            content: Some(content),
        };

        self.send("NEW", message.encode_to_vec());
    }

    async fn send_updated(
        &self,
        user_id: Uuid,
        id: ObjectId,
        seen: bool,
        timestamp: OffsetDateTime,
    ) {
        let message = NotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Updated.into(),
            timestamp: Some(Timestamp {
                seconds: timestamp.unix_timestamp(),
                nanos: timestamp.nanosecond() as i32,
            }),
            seen: Some(seen),
            content_type: None,
            content: None,
        };

        self.send("UPDATED", message.encode_to_vec());
    }

    async fn send_deleted(&self, user_id: Uuid, id: ObjectId, timestamp: OffsetDateTime) {
        let message = NotificationProtobuf {
            user_ids: vec![user_id.to_string()],
            id: id.to_hex(),
            status: NotificationStatusProtobuf::Deleted.into(),
            timestamp: Some(Timestamp {
                seconds: timestamp.unix_timestamp(),
                nanos: timestamp.nanosecond() as i32,
            }),
            seen: None,
            content_type: None,
            content: None,
        };

        self.send("DELETED", message.encode_to_vec());
    }
}
