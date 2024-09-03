use super::{dto::RabbitmqConsumerStatus, RabbitmqConsumerStatusChangeCallback};
use crate::{
    rabbitmq_consumer::rabbitmq_consumer_channel_callback::RabbitmqConsumerChannelCallback,
    retry::retry, RabbitmqConnection,
};
use amqprs::{
    channel::{
        BasicCancelArguments, BasicConsumeArguments, Channel, ExchangeDeclareArguments,
        QueueBindArguments, QueueDeclareArguments,
    },
    connection::Connection,
    consumer::AsyncConsumer,
};
use anyhow::anyhow;
use std::sync::Arc;
use tokio::sync::{watch, Notify};

pub struct RabbitmqConsumerStateMachine<Consumer, StatusCallback> {
    rabbitmq_connection: RabbitmqConnection,

    connection: Option<Connection>,
    connection_rx: watch::Receiver<Option<Connection>>,

    channel: Channel,
    exchange_declare_args: ExchangeDeclareArguments,
    queue_declare_args: QueueDeclareArguments,
    queue_bind_args: Vec<QueueBindArguments>,
    basic_consume_args: BasicConsumeArguments,
    consumer: Consumer,

    consumer_cancelled: Arc<Notify>,
    status_callback: StatusCallback,

    state: State,
}

impl<Consumer, StatusCallback> RabbitmqConsumerStateMachine<Consumer, StatusCallback>
where
    Consumer: AsyncConsumer + Clone + Send + 'static,
    StatusCallback: RabbitmqConsumerStatusChangeCallback + Send + 'static,
{
    pub fn new(
        rabbitmq_connection: RabbitmqConnection,
        connection: Connection,
        connection_rx: watch::Receiver<Option<Connection>>,
        channel: Channel,
        exchange_declare_args: ExchangeDeclareArguments,
        queue_declare_args: QueueDeclareArguments,
        queue_bind_args: Vec<QueueBindArguments>,
        basic_consume_args: BasicConsumeArguments,
        consumer: Consumer,
        consumer_cancelled: Arc<Notify>,
        status_callback: StatusCallback,
    ) -> Self {
        Self {
            rabbitmq_connection,
            connection: Some(connection),
            connection_rx,
            channel,
            exchange_declare_args,
            queue_declare_args,
            queue_bind_args,
            basic_consume_args,
            consumer,
            consumer_cancelled,
            status_callback,
            state: State::Ok,
        }
    }

    ///
    /// Infinite loop that keeps consumer alive.
    /// Loop can be stopped by using notify.
    ///
    #[tracing::instrument(
        name = "RabbitMQ Consumer",
        target = "rabbitmq_client::consumer",
        skip_all
    )]
    pub async fn run(mut self, stop: Arc<Notify>) {
        tracing::info!("state machine started");

        tokio::select! {
            biased;

            _ = stop.notified() => {
                tracing::info!("cancelling consumer");
                let args = BasicCancelArguments::new(&self.basic_consume_args.consumer_tag);
                match self.channel.basic_cancel(args).await {
                    Ok(_) => tracing::info!("consumer cancelled"),
                    Err(err) => tracing::warn!(%err, "cancelling consumer failed"),
                }

                tracing::info!("closing channel");
                match self.channel.close().await {
                    Ok(()) => tracing::info!("channel closed"),
                    Err(err) => tracing::warn!(%err, "closing channel failed"),
                }
            }

            _ = async { loop {
                match self.state {
                    State::Ok => {
                        tracing::info!("state: Ok");
                        self.ok_state().await;
                    }
                    State::WaitingForConnection => {
                        tracing::info!("state: WaitingForConnection");
                        self.waiting_for_connection_state().await;
                    }
                    State::RecreatingChannel => {
                        tracing::info!("state: RecreatingChannel");
                        self.recreating_channel_state().await;
                    }
                    State::RestoringConsumer => {
                        tracing::info!("state: RestoringConsumer");
                        self.restoring_consumer_state().await;
                    }
                }
            }} => {}
        }

        tracing::info!("state machine finished");
    }

    async fn ok_state(&mut self) {
        self.status_callback
            .execute(RabbitmqConsumerStatus::Consuming)
            .await;

        tokio::select! {
            biased;

            _ = self.connection_rx.changed() => {
                tracing::info!("connection changed");
                self.state = State::WaitingForConnection;
            }
            _ = self.consumer_cancelled.notified() => {
                tracing::info!("consumer got cancelled");
                self.state = State::RestoringConsumer;
            }
        }

        self.status_callback
            .execute(RabbitmqConsumerStatus::Recovering)
            .await;
    }

    async fn waiting_for_connection_state(&mut self) {
        loop {
            // Update connection before loop, because it's possible
            // that channel value has been changed again
            self.connection = self.connection_rx.borrow_and_update().clone();
            if self.connection.is_some() {
                break;
            }

            // it's safe because connection_tx cannot be dropped before dropping producer
            self.connection_rx.changed().await.unwrap();
        }

        self.state = State::RecreatingChannel;
    }

    async fn recreating_channel_state(&mut self) {
        // It's good to close channel just in case to prevent
        // resource leak. Failure is not a problem at this point
        tracing::info!("closing channel");
        match self.channel.clone().close().await {
            Ok(()) => tracing::info!("channel closed"),
            Err(err) => tracing::warn!(%err, "failed to close channel"),
        }

        tokio::select! {
            biased;

            _ = self.connection_rx.changed() => {
                tracing::info!("connection changed");
                self.state = State::WaitingForConnection;
            }

            _ = async {
                // It's not possible to reach this state with connection == None
                let connection = self.connection.as_ref().unwrap();

                self.channel = retry(
                    self.rabbitmq_connection.config().retry_interval,
                    |attempt| tracing::info!(attempt, "recreating channel"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to recreate channel"),
                    || async { connection.open_channel(None).await }
                )
                .await;

                self.consumer_cancelled = Arc::new(Notify::new());

                retry(
                    self.rabbitmq_connection.config().retry_interval,
                    |attempt| tracing::info!(attempt, "recreating channel callback"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to recreate channel callback"),
                    || async {
                        let consumer_cancelled = Arc::clone(&self.consumer_cancelled);
                        let channel_callback = RabbitmqConsumerChannelCallback::new(consumer_cancelled);
                        self
                            .channel
                            .register_callback(channel_callback).await
                    }
                )
                .await;
            } => {
                self.state = State::RestoringConsumer;
            }
        }
    }

    async fn restoring_consumer_state(&mut self) {
        tokio::select! {
            biased;

            _ = self.connection_rx.changed() => {
                tracing::info!("connection changed");
                self.state = State::WaitingForConnection;
            }

            result = Self::try_restore_consumer(
                &self.channel,
                self.exchange_declare_args.clone(),
                self.queue_declare_args.clone(),
                self.queue_bind_args.clone(),
                self.basic_consume_args.clone(),
                self.consumer.clone(),
            ) => {
                match result {
                    Ok(()) => self.state = State::Ok,
                    Err(err) => {
                        tracing::warn!(%err, "failed to restore consumer");
                        self.state = State::RecreatingChannel;
                    }
                }
            }
        }
    }

    async fn try_restore_consumer(
        channel: &Channel,
        exchange_declare_args: ExchangeDeclareArguments,
        queue_declare_args: QueueDeclareArguments,
        queue_bind_args: Vec<QueueBindArguments>,
        basic_consume_args: BasicConsumeArguments,
        consumer: Consumer,
    ) -> anyhow::Result<()> {
        tracing::info!("recreating exchange");
        channel
            .exchange_declare(exchange_declare_args)
            .await
            .map_err(|err| anyhow!("failed to recreate exchange: {err}"))?;

        tracing::info!("recreating queue");
        channel
            .queue_declare(queue_declare_args)
            .await
            .map_err(|err| anyhow!("failed to recreate queue: {err}"))?;

        tracing::info!("binding queue");
        for queue_bind_args in queue_bind_args {
            channel
                .queue_bind(queue_bind_args)
                .await
                .map_err(|err| anyhow!("failed to bind queue: {err}"))?;
        }

        tracing::info!("consuming");
        channel
            .basic_consume(consumer, basic_consume_args)
            .await
            .map_err(|err| anyhow!("failed to consume: {err}"))?;

        Ok(())
    }
}

enum State {
    Ok,
    WaitingForConnection,
    RecreatingChannel,
    RestoringConsumer,
}
