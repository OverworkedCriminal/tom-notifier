use super::{
    dto::{Message, PublisherConfirm},
    rabbitmq_producer_channel_callback::RabbitmqProducerChannelCallback,
};
use crate::{rabbitmq_producer::dto::PublisherConfirmVariant, retry::retry, RabbitmqConnection};
use amqprs::{
    channel::{BasicPublishArguments, Channel, ConfirmSelectArguments, ExchangeDeclareArguments},
    connection::Connection,
};
use std::collections::VecDeque;
use tokio::sync::{mpsc, watch};

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

    pub fn channel(&self) -> &Channel {
        &self.channel
    }

    ///
    /// Infinite loop that keeps producer alive.
    /// It's designed to work with external signal to stop it.
    /// ```text
    /// tokio::select! {
    ///     _ = notify.notified() => {}
    ///     _ = state_machine.run() => {}
    /// }
    /// ```
    ///
    pub async fn run(&mut self) {
        loop {
            match self.state {
                State::Ok => {
                    tracing::info!("producer state: Ok");
                    self.ok_state().await;
                }
                State::PreparingForConnection => {
                    tracing::info!("producer state: PreparingForConnection");
                    self.preparing_for_connection_state().await;
                }
                State::WaitingForConnection => {
                    tracing::info!("producer state: WaitingForConnection");
                    self.waiting_for_connection_state().await;
                }
                State::RestoringProducer => {
                    tracing::info!("producer state: RestoringProducer");
                    self.restoring_producer_state().await;
                }
            }
        }
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
                    self.state = State::PreparingForConnection;
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
                    let publish_result = self.channel
                        .basic_publish(
                            message.basic_properties.clone(),
                            message.content.clone(),
                            args,
                        )
                        .await;
                    match publish_result {
                        Ok(()) => {
                            self.unconfirmed_messages.push_back((message_count, message));
                            tracing::info!(message_count, "message processed");
                        },
                        Err(err) => {
                            tracing::warn!(message_count, %err, "basic publish failed");
                            // put message back to messages_tx to make sure it
                            // will be sent after recreating producer
                            self.messages_tx.send(message).unwrap();
                            self.state = State::PreparingForConnection;
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn preparing_for_connection_state(&mut self) {
        // Recreation of the producer involves creating a new channel
        // so it's good to close the old one.
        //
        // It will fail in most cases because this state is entered
        // after connection error,
        // but it's possible to enter this state after failed basic.publish.
        tracing::info!("closing channel");
        match self.channel.clone().close().await {
            Ok(()) => tracing::info!("channel closed"),
            Err(err) => tracing::warn!(%err, "failed to close channel"),
        }

        // Since connection failed there won't be any new confirms.
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

        self.state = State::WaitingForConnection;
    }

    async fn waiting_for_connection_state(&mut self) {
        // Update connection before loop, because it's possible
        // that channel value has been changed again
        self.connection = self.connection_rx.borrow_and_update().clone();

        while self.connection.is_none() {
            // it's safe because connection_tx cannot be dropped before dropping producer
            self.connection_rx.changed().await.unwrap();
            self.connection = self.connection_rx.borrow_and_update().clone();
        }

        self.state = State::RestoringProducer;
    }

    async fn restoring_producer_state(&mut self) {
        tokio::select! {
            biased;

            _ = self.connection_rx.changed() => {
                tracing::info!("connection changed");
                // confirmations are processed, but channel could have been
                // opened so its necessary to go back to PreparingForConnection
                self.state = State::PreparingForConnection;
            }

            _ = async {
                // It's not possible to reach this state with connection None
                let connection = self.connection.as_ref().unwrap();

                self.channel = retry(
                    self.rabbitmq_connection.config().retry_interval,
                    |attempt| tracing::info!(attempt, "recreating channel"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to recreate channel"),
                    || async { connection.open_channel(None).await },
                )
                .await;

                // Makes sure new channel's flow isn't false from the start
                self.flow_tx.send_replace(true);

                retry(
                    self.rabbitmq_connection.config().retry_interval,
                    |attempt| tracing::info!(attempt, "recreating channel callback"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to recreate channel callback"),
                    || async { self.channel.register_callback(self.channel_callback.clone()).await },
                )
                .await;

                retry(
                    self.rabbitmq_connection.config().retry_interval,
                    |attempt| tracing::info!(attempt, "recreating exchange"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to recreate exchange"),
                    || async {
                        self.channel
                            .exchange_declare(self.exchange_declare_args.clone())
                            .await
                    }
                )
                .await;

                retry(
                    self.rabbitmq_connection.config().retry_interval,
                    |attempt| tracing::info!(attempt, "enabling publisher confirms"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to enable publisher confirms"),
                    || async {
                        self.channel
                            .confirm_select(ConfirmSelectArguments::new(false))
                            .await
                    }
                )
                .await;
            } => {
                self.state = State::Ok;
            }
        }
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
    PreparingForConnection,
    WaitingForConnection,
    RestoringProducer,
}
