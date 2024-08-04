use super::{dto::Message, rabbitmq_producer_state_machine::RabbitmqProducerStateMachine};
use crate::{
    rabbitmq_producer::rabbitmq_producer_channel_callback::RabbitmqProducerChannelCallback,
    RabbitmqConnection,
};
use amqprs::{
    channel::{Channel, ConfirmSelectArguments, ExchangeDeclareArguments},
    BasicProperties,
};
use std::sync::Arc;
use tokio::{
    sync::{mpsc, watch, Notify},
    task::JoinHandle,
};

pub struct RabbitmqProducer {
    messages_tx: mpsc::UnboundedSender<Box<Message>>,

    task_handle: JoinHandle<Channel>,
    close_notify: Arc<Notify>,
}

impl RabbitmqProducer {
    #[tracing::instrument(
        name = "RabbitMQ Producer",
        target = "rabbitmq_client::producer",
        skip_all
    )]
    pub async fn new(
        rabbitmq_connection: RabbitmqConnection,
        mut exchange_declare_args: ExchangeDeclareArguments,
    ) -> anyhow::Result<Self> {
        tracing::info!("starting producer");

        let mut connection_rx = rabbitmq_connection.connection();
        let blocked_rx = rabbitmq_connection.connection_blocked();
        let Some(connection) = connection_rx.borrow_and_update().clone() else {
            anyhow::bail!("connection failed before creating producer");
        };

        tracing::info!("opening channel");
        let channel = connection.open_channel(None).await?;

        tracing::info!("registering channel callback");
        let (confirms_tx, confirms_rx) = mpsc::unbounded_channel();
        let (messages_tx, messages_rx) = mpsc::unbounded_channel();
        let (flow_tx, flow_rx) = watch::channel(true);
        let channel_callback = RabbitmqProducerChannelCallback::new(confirms_tx, flow_tx.clone());
        channel.register_callback(channel_callback.clone()).await?;

        tracing::info!("declaring exchange");
        exchange_declare_args.no_wait = false;
        channel
            .exchange_declare(exchange_declare_args.clone())
            .await?;

        tracing::info!("enabling publisher confirms");
        let args = ConfirmSelectArguments::new(false);
        channel.confirm_select(args).await?;

        let close_notify = Arc::new(Notify::new());

        let state_machine = RabbitmqProducerStateMachine::new(
            rabbitmq_connection,
            connection,
            channel,
            channel_callback,
            exchange_declare_args,
            connection_rx,
            messages_tx.clone(),
            messages_rx,
            confirms_rx,
            flow_tx,
            flow_rx,
            blocked_rx,
        );

        let task_handle = tokio::spawn(keep_alive(close_notify.clone(), state_machine));

        tracing::info!("producer started");

        Ok(Self {
            messages_tx,
            task_handle,
            close_notify,
        })
    }

    #[tracing::instrument(
        name = "RabbitMQ Producer",
        target = "rabbitmq_client::producer",
        skip_all
    )]
    pub async fn close(self) {
        tracing::info!("closing producer");

        self.close_notify.notify_one();

        // task cannot fail/panic
        let channel = self.task_handle.await.unwrap();

        tracing::info!("closing channel");
        if let Err(err) = channel.close().await {
            tracing::warn!(%err, "closing channel failed");
        }

        tracing::info!("producer closed");
    }

    pub fn send(&self, routing_key: String, basic_properties: BasicProperties, content: Vec<u8>) {
        let message = Box::new(Message {
            routing_key,
            basic_properties,
            content,
        });

        // messages_rx always exist in external task that
        self.messages_tx.send(message).unwrap();
    }
}

///
/// Runs producer state machine.
/// Since state machine works in infinite loop this function
/// should be called in external task.
///
/// ### Returns
/// Channel used by state machine. There's no guarantee it will be still opened
///
#[tracing::instrument(
    name = "RabbitMQ Producer",
    target = "rabbitmq_client::producer",
    skip_all
)]
async fn keep_alive(
    close_notify: Arc<Notify>,
    mut state_machine: RabbitmqProducerStateMachine,
) -> Channel {
    tracing::info!("keep alive started");

    tokio::select! {
        biased;

        _ = close_notify.notified() => {}
        _ = state_machine.run() => {}
    }

    tracing::info!("keep alive finished");

    state_machine.channel().clone()
}
