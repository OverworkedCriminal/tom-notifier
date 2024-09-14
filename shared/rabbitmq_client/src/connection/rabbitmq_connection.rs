use super::dto::RabbitmqConnectionConfig;
use crate::connection::{connection_callback::ConnectionCallback, state_machine::StateMachine};
use amqprs::connection::{Connection, OpenConnectionArguments};
use std::sync::Arc;
use tokio::{
    sync::{watch, Notify},
    task::JoinHandle,
};

///
/// RabbitMQ connection.
/// It runs background task that recreates connection whenever io_failure occurs.
///
/// Underlying connection can be accessed by [Self::connection].
/// Blocked signal can be accesed by [Self::connection_blocked].
///
#[derive(Clone)]
pub struct RabbitmqConnection {
    inner: Arc<RabbitmqConnectionInner>,
}

struct RabbitmqConnectionInner {
    config: RabbitmqConnectionConfig,

    connection_rx: watch::Receiver<Option<Connection>>,
    connection_blocked_rx: watch::Receiver<bool>,

    keep_alive_handle: JoinHandle<()>,
    close_notify: Arc<Notify>,
}

impl RabbitmqConnection {
    #[tracing::instrument(
        name = "RabbitMQ Connection",
        target = "rabbitmq_client::connection",
        skip_all
    )]
    pub async fn new(
        config: RabbitmqConnectionConfig,
        open_connection_args: OpenConnectionArguments,
    ) -> Result<Self, amqprs::error::Error> {
        tracing::info!("opening connection");
        let connection = Connection::open(&open_connection_args).await?;

        tracing::info!("registering callback");
        let (blocked_tx, blocked_rx) = watch::channel(false);
        let callback = ConnectionCallback::new(blocked_tx.clone());
        connection.register_callback(callback.clone()).await?;

        tracing::info!("starting keep alive task");
        let close_notify = Arc::new(Notify::new());
        let (connection_tx, connection_rx) = watch::channel(Some(connection.clone()));
        let state_machine = StateMachine::new(
            config.clone(),
            connection,
            connection_tx,
            open_connection_args,
            callback,
            blocked_tx,
        );

        let close_notify_clone = Arc::clone(&close_notify);
        let keep_alive_handle = tokio::spawn(async move {
            state_machine.run(close_notify_clone).await;
        });

        tracing::info!("connection opened");

        Ok(Self {
            inner: Arc::new(RabbitmqConnectionInner {
                config,
                connection_rx,
                connection_blocked_rx: blocked_rx,
                keep_alive_handle,
                close_notify,
            }),
        })
    }

    ///
    /// Close underlying connection and task that recreates it.
    ///
    /// ### Errors
    /// Returns an error when it is not the last clone of the connection
    ///
    #[tracing::instrument(
        name = "RabbitMQ Connection",
        target = "rabbitmq_client::connection",
        skip_all
    )]
    pub async fn close(self) {
        let Ok(inner) = Arc::try_unwrap(self.inner) else {
            tracing::error!("closing connection when connection clones exist is forbidden");
            return;
        };

        inner.close_notify.notify_one();
        inner.keep_alive_handle.await.unwrap(); // task can't be aborted and will never panic
    }

    pub fn config(&self) -> &RabbitmqConnectionConfig {
        &self.inner.config
    }

    pub fn connection(&self) -> watch::Receiver<Option<Connection>> {
        self.inner.connection_rx.clone()
    }

    pub fn connection_blocked(&self) -> watch::Receiver<bool> {
        self.inner.connection_blocked_rx.clone()
    }
}
