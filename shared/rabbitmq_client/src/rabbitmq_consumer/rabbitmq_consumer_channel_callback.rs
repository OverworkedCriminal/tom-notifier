use amqprs::{
    callbacks::ChannelCallback, channel::Channel, Ack, BasicProperties, Cancel, CloseChannel, Nack,
    Return,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Notify;

pub struct RabbitmqConsumerChannelCallback {
    consumer_cancelled_notify: Arc<Notify>,
}

impl RabbitmqConsumerChannelCallback {
    pub fn new(consumer_cancelled_notify: Arc<Notify>) -> Self {
        Self {
            consumer_cancelled_notify,
        }
    }
}

#[async_trait]
impl ChannelCallback for RabbitmqConsumerChannelCallback {
    #[tracing::instrument(
        name = "RabbitMQ Consumer Callback",
        target = "rabbitmq_client::consumer_callback",
        skip_all
    )]
    async fn close(
        &mut self,
        _channel: &Channel,
        close: CloseChannel,
    ) -> Result<(), amqprs::error::Error> {
        tracing::error!(
            code = close.reply_code(),
            text = close.reply_text(),
            "received close",
        );

        Ok(())
    }

    #[tracing::instrument(
        name = "RabbitMQ Consumer Callback",
        target = "rabbitmq_client::consumer_callback",
        skip_all
    )]
    async fn cancel(
        &mut self,
        _channel: &Channel,
        _cancel: Cancel,
    ) -> Result<(), amqprs::error::Error> {
        tracing::error!("received cancel");

        self.consumer_cancelled_notify.notify_one();

        Ok(())
    }

    async fn flow(
        &mut self,
        _channel: &Channel,
        active: bool,
    ) -> Result<bool, amqprs::error::Error> {
        // NOP this channel won't be used for publishing
        Ok(active)
    }

    async fn publish_ack(&mut self, _channel: &Channel, _ack: Ack) {
        // NOP this channel won't be used for publishing
    }

    async fn publish_nack(&mut self, _channel: &Channel, _nack: Nack) {
        // NOP this channel won't be used for publishing
    }

    async fn publish_return(
        &mut self,
        _channel: &Channel,
        _ret: Return,
        _basic_properties: BasicProperties,
        _content: Vec<u8>,
    ) {
        // NOP this channel won't be used for publishing
    }
}
