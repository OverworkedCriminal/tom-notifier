use super::{
    dto::{Message, PublisherConfirm},
    rabbitmq_producer_channel_callback::RabbitmqProducerChannelCallback,
};
use crate::{rabbitmq_producer::dto::PublisherConfirmVariant, retry::retry, connection::RabbitmqConnection};
use amqprs::{
    channel::{BasicPublishArguments, Channel, ConfirmSelectArguments, ExchangeDeclareArguments},
    connection::Connection,
};
use anyhow::anyhow;
use std::{collections::VecDeque, sync::Arc};
use tokio::sync::{mpsc, watch, Notify};

pub struct RabbitmqProducerStateMachine {
    rabbitmq_connection: RabbitmqConnection,

    connection: Option<Connection>,
    connection_rx: watch::Receiver<Option<Connection>>,

    channel: Channel,
    channel_callback: RabbitmqProducerChannelCallback,

    exchange_declare_args: ExchangeDeclareArguments,

    unconfirmed_messages: VecDeque<(u64, Box<Message>)>,
    messages_tx: mpsc::UnboundedSender<Box<Message>>,
    messages_rx: mpsc::UnboundedReceiver<Box<Message>>,

    confirms_rx: mpsc::UnboundedReceiver<PublisherConfirm>,

    flow_tx: watch::Sender<bool>,
    flow_rx: watch::Receiver<bool>,

    blocked_rx: watch::Receiver<bool>,

    state: State,
}

impl RabbitmqProducerStateMachine {
    pub fn new(
        rabbitmq_connection: RabbitmqConnection,
        connection: Connection,
        channel: Channel,
        channel_callback: RabbitmqProducerChannelCallback,
        exchange_declare_args: ExchangeDeclareArguments,
        connection_rx: watch::Receiver<Option<Connection>>,
        messages_tx: mpsc::UnboundedSender<Box<Message>>,
        messages_rx: mpsc::UnboundedReceiver<Box<Message>>,
        confirms_rx: mpsc::UnboundedReceiver<PublisherConfirm>,
        flow_tx: watch::Sender<bool>,
        flow_rx: watch::Receiver<bool>,
        blocked_rx: watch::Receiver<bool>,
    ) -> Self {
        Self {
            rabbitmq_connection,
            connection: Some(connection),
            channel,
            channel_callback,
            exchange_declare_args,
            connection_rx,
            unconfirmed_messages: VecDeque::new(),
            messages_tx,
            messages_rx,
            confirms_rx,
            flow_tx,
            flow_rx,
            blocked_rx,
            state: State::Ok,
        }
    }

    ///
    /// Infinite loop that keeps producer alive.
    /// Loop can be stopped by using notify.
    ///
    #[tracing::instrument(
        name = "RabbitMQ Producer",
        target = "rabbitmq_client::producer",
        skip_all
    )]
    pub async fn run(mut self, stop: Arc<Notify>) {
        tracing::info!("state machine started");

        tokio::select! {
            biased;
            _ = stop.notified() => {
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
                    State::ProcessingPendingConfirmations => {
                        tracing::info!("state: ProcessingPendingConfirmations");
                        self.processing_pending_confirmations_state().await;
                    }
                    State::RecreatingChannel => {
                        tracing::info!("state: RecreatingChannel");
                        self.recreating_channel_state().await;
                    }
                    State::RestoringProducer => {
                        tracing::info!("state: RestoringProducer");
                        self.restoring_producer_state().await;
                    }
                }
            }} => {}
        }

        tracing::info!("state machine finished");
    }

    async fn ok_state(&mut self) {
        let mut flow = *self.flow_rx.borrow_and_update();
        let mut blocked = *self.blocked_rx.borrow_and_update();

        // counter meant to track which message was confirmed
        // by publisher confirms
        let mut message_count = 0;

        loop {
            tokio::select! {
                biased;

                _ = self.connection_rx.changed() => {
                    tracing::info!("connection changed");
                    self.state = State::WaitingForConnection;
                    break;
                }

                _ = self.flow_rx.changed() => {
                    flow = *self.flow_rx.borrow_and_update();
                    tracing::debug!(flow, "flow changed");
                }

                _ = self.blocked_rx.changed() => {
                    blocked = *self.blocked_rx.borrow_and_update();
                    tracing::debug!(blocked, "blocked changed");
                }

                result = self.confirms_rx.recv() => {
                    // channel cannot be closed because
                    // there is callback clone within this function
                    let confirm = result.unwrap();
                    self.process_confirm(confirm);
                }

                result = self.messages_rx.recv(), if !blocked && flow => {
                    tracing::info!("processing message");

                    // channel cannot be closed because
                    // state machine holds both ends of the channel
                    let message = result.unwrap();
                    message_count += 1;

                    let args = BasicPublishArguments {
                        exchange: self.exchange_declare_args.exchange.clone(),
                        routing_key: message.routing_key.clone(),
                        mandatory: false,
                        immediate: false,
                    };
                    match self.channel
                        .basic_publish(message.basic_properties.clone(), message.content.clone(), args)
                        .await
                    {
                        Ok(()) => {
                            self.unconfirmed_messages.push_back((message_count, message));
                            tracing::info!(message_count, "message processed");
                        },
                        Err(err) => {
                            tracing::warn!(message_count, %err, "basic publish failed");
                            // put message back to messages_tx to make sure it
                            // will be sent after recreating producer
                            self.messages_tx.send(message).unwrap();
                            self.state = State::ProcessingPendingConfirmations;
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn waiting_for_connection_state(&mut self) {
        loop {
            self.connection = self.connection_rx.borrow_and_update().clone();
            if self.connection.is_some() {
                break;
            }

            // it's safe because connection_tx cannot be dropped before dropping producer
            self.connection_rx.changed().await.unwrap();
        }

        self.state = State::ProcessingPendingConfirmations;
    }

    async fn processing_pending_confirmations_state(&mut self) {
        // Since connection or channel failed there won't be any new confirms.
        // It means it's possible to clear channel here.
        tracing::info!("processing remaining confirmations");
        while !self.confirms_rx.is_empty() {
            // confirms_rx cannot be closed as long as channel callback clone lives
            let confirm = self.confirms_rx.recv().await.unwrap();
            self.process_confirm(confirm);
        }

        // Schedule all unconfirmed messages for another send
        tracing::info!("scheduling unconfirmed messages to be resent");
        while let Some((message_count, message)) = self.unconfirmed_messages.pop_front() {
            tracing::trace!(message_count, "unconfirmed message scheduled to be resent");
            self.messages_tx.send(message).unwrap();
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

                // Makes sure new channel's flow isn't false from the start
                self.flow_tx.send_replace(true);

                retry(
                    self.rabbitmq_connection.config().retry_interval,
                    |attempt| tracing::info!(attempt, "recreating channel callback"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to recreate channel callback"),
                    || async { self.channel.register_callback(self.channel_callback.clone()).await }
                )
                .await;
            } => {
                self.state = State::RestoringProducer
            }
        }
    }

    async fn restoring_producer_state(&mut self) {
        tokio::select! {
            biased;

            _ = self.connection_rx.changed() => {
                tracing::info!("connection changed");
                self.state = State::WaitingForConnection;
            }

            result = Self::try_restore_producer(&self.channel, self.exchange_declare_args.clone()) => {
                match result {
                    Ok(()) => self.state = State::Ok,
                    Err(err) => {
                        tracing::warn!(%err, "failed to recreate producer");
                        self.state = State::RecreatingChannel;
                    }
                }
            }
        }
    }

    async fn try_restore_producer(
        channel: &Channel,
        exchange_declare_args: ExchangeDeclareArguments,
    ) -> anyhow::Result<()> {
        tracing::info!("recreating exchange");
        channel
            .exchange_declare(exchange_declare_args)
            .await
            .map_err(|err| anyhow!("failed to recreate exchange: {err}"))?;

        tracing::info!("enabling publisher confirms");
        channel
            .confirm_select(ConfirmSelectArguments::new(false))
            .await
            .map_err(|err| anyhow!("failed to enable publisher confirms: {err}"))?;

        Ok(())
    }

    fn process_confirm(&mut self, confirm: PublisherConfirm) {
        tracing::debug!(
            delivery_tag = confirm.delivery_tag,
            multiple = confirm.multiple,
            "processing publisher confirm"
        );

        let on_remove_ack = |_message_count, _message| {};
        let on_remove_nack = |message_count, message| {
            self.messages_tx.send(message).unwrap();
            tracing::trace!(message_count, "nacked message scheduled to be resent");
        };
        let on_remove: &dyn Fn(u64, Box<Message>) = match confirm.variant {
            PublisherConfirmVariant::Ack => &on_remove_ack,
            PublisherConfirmVariant::Nack => &on_remove_nack,
        };

        match confirm.multiple {
            true => {
                // Messages in queue are ordered by message_count
                // so removing messages from the front is okay
                while let Some((message_count, _)) = self.unconfirmed_messages.front() {
                    match *message_count <= confirm.delivery_tag {
                        true => {
                            let (message_count, message) =
                                unsafe { self.unconfirmed_messages.pop_front().unwrap_unchecked() };
                            on_remove(message_count, message);
                        }
                        false => break,
                    }
                }
            }
            false => {
                match self
                    .unconfirmed_messages
                    .iter()
                    .position(|(message_count, _)| *message_count == confirm.delivery_tag)
                {
                    Some(idx) => {
                        let (message_count, message) =
                            unsafe { self.unconfirmed_messages.remove(idx).unwrap_unchecked() };
                        on_remove(message_count, message);
                    }
                    None => tracing::trace!(
                        message_count = confirm.delivery_tag,
                        "message already confirmed"
                    ),
                }
            }
        }

        tracing::debug!(
            delivery_tag = confirm.delivery_tag,
            multiple = confirm.multiple,
            "publisher confirm processed"
        );
    }
}

enum State {
    Ok,
    WaitingForConnection,
    ProcessingPendingConfirmations,
    RecreatingChannel,
    RestoringProducer,
}
