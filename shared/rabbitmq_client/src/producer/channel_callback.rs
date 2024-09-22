use super::dto::PublisherConfirm;
use crate::producer::dto::PublisherConfirmVariant;
use amqprs::{channel::Channel, Ack, BasicProperties, Cancel, CloseChannel, Nack, Return};
use async_trait::async_trait;
use tokio::sync::{mpsc, watch};

#[derive(Clone)]
pub struct ChannelCallback {
    publisher_confirm_tx: mpsc::UnboundedSender<PublisherConfirm>,
    flow_tx: watch::Sender<bool>,
}

impl ChannelCallback {
    pub fn new(
        publisher_confirm_tx: mpsc::UnboundedSender<PublisherConfirm>,
        flow_tx: watch::Sender<bool>,
    ) -> Self {
        Self {
            publisher_confirm_tx,
            flow_tx,
        }
    }
}

#[async_trait]
impl amqprs::callbacks::ChannelCallback for ChannelCallback {
    #[tracing::instrument(
        name = "RabbitMQ Producer Callback",
        target = "rabbitmq_client::producer_callback",
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

    async fn cancel(
        &mut self,
        _channel: &Channel,
        _cancel: Cancel,
    ) -> Result<(), amqprs::error::Error> {
        Ok(())
    }

    #[tracing::instrument(
        name = "RabbitMQ Producer Callback",
        target = "rabbitmq_client::producer_callback",
        skip_all
    )]
    async fn flow(
        &mut self,
        _channel: &Channel,
        active: bool,
    ) -> Result<bool, amqprs::error::Error> {
        tracing::trace!(flow = active, "received flow");

        self.flow_tx.send_replace(active);

        Ok(active)
    }

    #[tracing::instrument(
        name = "RabbitMQ Producer Callback",
        target = "rabbitmq_client::producer_callback",
        skip_all
    )]
    async fn publish_ack(&mut self, _channel: &Channel, ack: Ack) {
        tracing::trace!(
            delivery_tag = ack.delivery_tag(),
            multiple = ack.mutiple(),
            "received ack"
        );

        let publisher_confirm = PublisherConfirm {
            delivery_tag: ack.delivery_tag(),
            multiple: ack.mutiple(),
            variant: PublisherConfirmVariant::Ack,
        };
        if let Err(_) = self.publisher_confirm_tx.send(publisher_confirm) {
            tracing::error!("publisher_confirm channel closed");
        }
    }

    #[tracing::instrument(
        name = "RabbitMQ Producer Callback",
        target = "rabbitmq_client::producer_callback",
        skip_all
    )]
    async fn publish_nack(&mut self, _channel: &Channel, nack: Nack) {
        tracing::trace!(
            delivery_tag = nack.delivery_tag(),
            multiple = nack.multiple(),
            "received nack"
        );

        let publisher_confirm = PublisherConfirm {
            delivery_tag: nack.delivery_tag(),
            multiple: nack.multiple(),
            variant: PublisherConfirmVariant::Nack,
        };
        if let Err(_) = self.publisher_confirm_tx.send(publisher_confirm) {
            tracing::error!("publisher_confirm channel closed");
        }
    }

    async fn publish_return(
        &mut self,
        _channel: &Channel,
        _ret: Return,
        _basic_properties: BasicProperties,
        _content: Vec<u8>,
    ) {
    }
}
