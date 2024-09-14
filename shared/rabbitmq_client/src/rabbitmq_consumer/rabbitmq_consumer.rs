use super::{
    rabbitmq_consumer_state_machine::RabbitmqConsumerStateMachine,
    RabbitmqConsumerStatusChangeCallback,
};
use crate::{
    rabbitmq_consumer::rabbitmq_consumer_channel_callback::RabbitmqConsumerChannelCallback,
    connection::RabbitmqConnection,
};
use amqprs::{
    channel::{
        BasicConsumeArguments, ExchangeDeclareArguments, QueueBindArguments, QueueDeclareArguments,
    },
    consumer::AsyncConsumer,
};
use std::sync::Arc;
use tokio::{sync::Notify, task::JoinHandle};

pub struct RabbitmqConsumer {
    task_handle: JoinHandle<()>,

    close_notify: Arc<Notify>,
}

impl RabbitmqConsumer {
    #[tracing::instrument(
        name = "RabbitMQ Consumer",
        target = "rabbitmq_client::consumer",
        skip_all
    )]
    pub async fn new<Consumer, StatusCallback>(
        rabbitmq_connection: RabbitmqConnection,
        mut exchange_declare_args: ExchangeDeclareArguments,
        mut queue_declare_args: QueueDeclareArguments,
        mut queue_bind_args: Vec<QueueBindArguments>,
        mut basic_consume_args: BasicConsumeArguments,
        consumer: Consumer,
        status_callback: StatusCallback,
    ) -> anyhow::Result<Self>
    where
        Consumer: AsyncConsumer + Clone + Send + 'static,
        StatusCallback: RabbitmqConsumerStatusChangeCallback + Send + 'static,
    {
        tracing::info!("starting consumer");

        let mut connection_rx = rabbitmq_connection.connection();
        let Some(connection) = connection_rx.borrow_and_update().clone() else {
            anyhow::bail!("connection failed before creating producer");
        };

        tracing::info!("opening channel");
        let channel = connection.open_channel(None).await?;

        tracing::info!("registering channel callback");
        let consumer_cancelled = Arc::new(Notify::new());
        let consumer_cancelled_clone = Arc::clone(&consumer_cancelled);
        let channel_callback = RabbitmqConsumerChannelCallback::new(consumer_cancelled_clone);
        channel.register_callback(channel_callback).await?;

        tracing::info!("declaring exchange");
        exchange_declare_args.no_wait = false;
        channel
            .exchange_declare(exchange_declare_args.clone())
            .await?;

        tracing::info!("declaring queue");
        queue_declare_args.no_wait(false);
        channel.queue_declare(queue_declare_args.clone()).await?;

        tracing::info!("binding queue");
        for queue_bind_args in queue_bind_args.iter_mut() {
            queue_bind_args.no_wait = false;
            channel.queue_bind(queue_bind_args.clone()).await?;
        }

        tracing::info!("consuming");
        basic_consume_args.no_wait = false;
        channel
            .basic_consume(consumer.clone(), basic_consume_args.clone())
            .await?;

        let state_machine = RabbitmqConsumerStateMachine::new(
            rabbitmq_connection,
            connection,
            connection_rx,
            channel,
            exchange_declare_args,
            queue_declare_args,
            queue_bind_args,
            basic_consume_args,
            consumer,
            consumer_cancelled,
            status_callback,
        );

        let close_notify = Arc::new(Notify::new());
        let close_notify_clone = Arc::clone(&close_notify);
        let task_handle = tokio::spawn(async move {
            state_machine.run(close_notify_clone).await;
        });

        tracing::info!("consumer started");

        Ok(Self {
            task_handle,
            close_notify,
        })
    }

    pub async fn close(self) {
        tracing::info!("closing consumer");

        self.close_notify.notify_one();

        // task cannot fail/panic
        self.task_handle.await.unwrap();

        tracing::info!("consumer closed");
    }
}
