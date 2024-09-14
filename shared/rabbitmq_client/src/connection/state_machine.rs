use super::{connection_callback::ConnectionCallback, RabbitmqConnectionConfig};
use crate::retry::retry;
use amqprs::connection::{Connection, OpenConnectionArguments};
use std::sync::Arc;
use tokio::sync::{watch, Notify};

pub struct StateMachine {
    config: RabbitmqConnectionConfig,

    connection: Connection,
    connection_tx: watch::Sender<Option<Connection>>,

    open_connection_args: OpenConnectionArguments,
    connection_callback: ConnectionCallback,

    blocked_tx: watch::Sender<bool>,

    state: State,
}

impl StateMachine {
    pub fn new(
        config: RabbitmqConnectionConfig,
        connection: Connection,
        connection_tx: watch::Sender<Option<Connection>>,
        open_connection_args: OpenConnectionArguments,
        connection_callback: ConnectionCallback,
        blocked_tx: watch::Sender<bool>,
    ) -> Self {
        Self {
            config,
            connection,
            connection_tx,
            open_connection_args,
            connection_callback,
            blocked_tx,
            state: State::Ok,
        }
    }

    ///
    /// Infinite loop that keeps connection alive.
    /// Loop can be stopped by using notify.
    ///
    #[tracing::instrument(
        name = "RabbitMQ Connection",
        target = "rabbitmq_client::connection",
        skip_all
    )]
    pub async fn run(mut self, stop: Arc<Notify>) {
        tracing::info!("state machine started");

        tokio::select! {
            biased;
            _ = stop.notified() => {
                tracing::info!("closing connection");
                match self.connection.close().await {
                    Ok(()) => tracing::info!("connection closed"),
                    Err(err) => tracing::warn!(%err, "closing connection failed"),
                }
            }
            _ = async { loop {
                match self.state {
                    State::Ok => {
                        tracing::info!("state: Ok");
                        self.ok_state().await;
                    }
                    State::ClosingConnection => {
                        tracing::info!("state: ClosingConnection");
                        self.closing_connection_state().await;
                    }
                    State::RestoringConnection => {
                        tracing::info!("state: RestoringConnection");
                        self.restoring_connection_state().await
                    }
                    State::RestoringCallback => {
                        tracing::info!("state: RestoringCallback");
                        self.restoring_callback_state().await
                    }
                }
            }} => {}
        }

        tracing::info!("state machine finished");
    }

    async fn ok_state(&mut self) {
        self.connection.listen_network_io_failure().await;
        tracing::warn!("connection failure");

        self.state = State::ClosingConnection;
    }

    async fn closing_connection_state(&mut self) {
        self.connection_tx.send_replace(None);

        match self.connection.clone().close().await {
            Ok(()) => tracing::info!("connection closed"),
            Err(err) => tracing::warn!(%err, "failed to close connection"),
        }

        self.state = State::RestoringConnection;
    }

    async fn restoring_connection_state(&mut self) {
        self.connection = retry(
            self.config.retry_interval,
            |attempt| tracing::info!(attempt, "recreating connection"),
            |attempt, err| tracing::warn!(attempt, %err, "failed to recreate connection"),
            || async { Connection::open(&self.open_connection_args).await },
        )
        .await;
        tracing::info!("connection recreated");

        // Makes sure new connection isn't blocked from the start
        self.blocked_tx.send_replace(false);

        // Sending connection through connection_tx is delayed
        // until connection is fully recreated (until callback is registered)

        self.state = State::RestoringCallback;
    }

    async fn restoring_callback_state(&mut self) {
        tokio::select! {
            // It's possible connection connection fails during recreating callbacks
            _ = self.connection.listen_network_io_failure() => {
                tracing::warn!("connection failed");
                self.state = State::ClosingConnection;
            }

            _ = async {
                retry(
                    self.config.retry_interval,
                    |attempt| tracing::info!(attempt, "recreating callback"),
                    |attempt, err| tracing::warn!(attempt, %err, "failed to recreate callback"),
                    || async {
                        self.connection.register_callback(self.connection_callback.clone()).await
                    },
                ).await;
                tracing::info!("callback recreated");
            } => {
                // Sending connection through connection_tx
                // because connection is ready to use again
                self.connection_tx.send_replace(Some(self.connection.clone()));
                self.state = State::Ok;
            }
        }
    }
}

enum State {
    Ok,
    ClosingConnection,
    RestoringConnection,
    RestoringCallback,
}
