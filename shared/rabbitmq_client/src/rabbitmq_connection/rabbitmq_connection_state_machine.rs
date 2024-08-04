use super::{rabbitmq_connection_callback::RabbitmqConnectionCallback, RabbitmqConnectionConfig};
use crate::retry::retry;
use amqprs::connection::{Connection, OpenConnectionArguments};
use tokio::sync::watch;

pub struct RabbitmqConnectionStateMachine {
    config: RabbitmqConnectionConfig,

    connection: Connection,
    connection_tx: watch::Sender<Option<Connection>>,

    open_connection_args: OpenConnectionArguments,
    connection_callback: RabbitmqConnectionCallback,

    blocked_tx: watch::Sender<bool>,

    state: State,
}

impl RabbitmqConnectionStateMachine {
    pub fn new(
        config: RabbitmqConnectionConfig,
        connection: Connection,
        connection_tx: watch::Sender<Option<Connection>>,
        open_connection_args: OpenConnectionArguments,
        connection_callback: RabbitmqConnectionCallback,
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
    /// It's designed to work with external signal to stop it.
    /// ```ignore
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
                    tracing::info!("connection state: Ok");
                    self.ok_state().await
                }
                State::RestoringConnection => {
                    tracing::info!("connection state: RestoringConnection");
                    self.restoring_connection_state().await
                }
                State::RestoringCallback => {
                    tracing::info!("connection state: RestoringCallback");
                    self.restoring_callback_state().await
                }
            }
        }
    }

    async fn ok_state(&mut self) {
        self.connection.listen_network_io_failure().await;
        tracing::warn!("connection broken");

        self.connection_tx.send_replace(None);

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
                tracing::warn!("connection broken");
                self.state = State::RestoringConnection;
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
    RestoringConnection,
    RestoringCallback,
}
