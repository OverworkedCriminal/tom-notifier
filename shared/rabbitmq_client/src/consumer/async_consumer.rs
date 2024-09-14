use super::{
    callback::RabbitmqConsumerDeliveryCallback, dto::DeliveryResponse, error::ConsumeError,
};
use amqprs::{
    channel::{BasicAckArguments, BasicNackArguments, Channel},
    AmqpDeliveryTag, BasicProperties, Deliver,
};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

pub struct AsyncConsumer<DeliveryCallback> {
    delivery_callback: Arc<DeliveryCallback>,

    response_tx: mpsc::UnboundedSender<DeliveryResponse>,
}

impl<DeliveryCallback> AsyncConsumer<DeliveryCallback> {
    pub fn new(channel: Channel, delivery_callback: Arc<DeliveryCallback>) -> Self {
        let (response_tx, response_rx) = mpsc::unbounded_channel();

        let response_task = ResponseTask::new(channel, response_rx);
        tokio::spawn(response_task.run());

        Self {
            delivery_callback,
            response_tx,
        }
    }
}

#[async_trait]
impl<DeliveryCallback> amqprs::consumer::AsyncConsumer for AsyncConsumer<DeliveryCallback>
where
    DeliveryCallback: RabbitmqConsumerDeliveryCallback + Send + Sync + 'static,
{
    #[tracing::instrument(
        name = "RabbitMQ Consumer",
        target = "rabbitmq_client::consumer",
        skip_all
    )]
    async fn consume(
        &mut self,
        _channel: &Channel,
        deliver: Deliver,
        _basic_properties: BasicProperties,
        content: Vec<u8>,
    ) {
        let delivery_tag = deliver.delivery_tag();

        tracing::info!(delivery_tag, "received delivery");

        let processing_task = ProcessingTask::new(
            Arc::clone(&self.delivery_callback),
            self.response_tx.downgrade(),
        );
        tokio::spawn(processing_task.run(delivery_tag, content));
    }
}

struct ResponseTask {
    channel: Channel,
    response_rx: mpsc::UnboundedReceiver<DeliveryResponse>,
}

impl ResponseTask {
    fn new(channel: Channel, response_rx: mpsc::UnboundedReceiver<DeliveryResponse>) -> Self {
        Self {
            channel,
            response_rx,
        }
    }

    #[tracing::instrument(
        name = "RabbitMQ Consumer",
        target = "rabbitmq_client::consumer",
        skip_all
    )]
    async fn run(mut self) {
        tracing::info!("consumer response task started");

        while let Some(response) = self.response_rx.recv().await {
            match response {
                DeliveryResponse::Ack { delivery_tag } => {
                    let args = BasicAckArguments::new(delivery_tag, false);
                    match self.channel.basic_ack(args).await {
                        Ok(()) => tracing::trace!(delivery_tag, "ack sent"),
                        Err(err) => tracing::warn!(delivery_tag, %err, "failed to send ack"),
                    }
                }
                DeliveryResponse::Nack {
                    delivery_tag,
                    requeue,
                } => {
                    let args = BasicNackArguments::new(delivery_tag, false, requeue);
                    match self.channel.basic_nack(args).await {
                        Ok(()) => tracing::trace!(delivery_tag, requeue, "nack sent"),
                        Err(err) => {
                            tracing::warn!(delivery_tag, requeue, %err, "failed to send nack")
                        }
                    }
                }
            }
        }

        tracing::info!("consumer response task finished");
    }
}

struct ProcessingTask<DeliveryCallback> {
    delivery_callback: Arc<DeliveryCallback>,
    response_tx: mpsc::WeakUnboundedSender<DeliveryResponse>,
}

impl<DeliveryCallback> ProcessingTask<DeliveryCallback>
where
    DeliveryCallback: RabbitmqConsumerDeliveryCallback,
{
    fn new(
        delivery_callback: Arc<DeliveryCallback>,
        response_tx: mpsc::WeakUnboundedSender<DeliveryResponse>,
    ) -> Self {
        Self {
            delivery_callback,
            response_tx,
        }
    }

    #[tracing::instrument(
        name = "RabbitMQ Consumer Processor",
        target = "rabbitmq_client::consumer",
        skip(self, content)
    )]
    async fn run(self, delivery_tag: AmqpDeliveryTag, content: Vec<u8>) {
        tracing::info!("processing delivery");

        let callback_result = self.delivery_callback.execute(content).await;
        let response = match callback_result {
            Ok(()) => DeliveryResponse::Ack { delivery_tag },
            Err(ConsumeError { requeue }) => DeliveryResponse::Nack {
                delivery_tag,
                requeue,
            },
        };

        // It's not an error if channel is closed.
        // No log here, because output thread will log when channel closes.
        if let Some(response_tx) = self.response_tx.upgrade() {
            let _ = response_tx.send(response);
        }
    }
}
