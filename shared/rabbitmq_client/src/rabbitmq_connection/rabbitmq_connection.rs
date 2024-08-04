use super::dto::RabbitmqConnectionConfig;
use crate::rabbitmq_connection::{
    rabbitmq_connection_callback::RabbitmqConnectionCallback,
    rabbitmq_connection_state_machine::RabbitmqConnectionStateMachine,
};
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
        let callback = RabbitmqConnectionCallback::new(blocked_tx.clone());
        connection.register_callback(callback.clone()).await?;

        tracing::info!("starting keep alive task");
        let close_notify = Arc::new(Notify::new());
        let (connection_tx, connection_rx) = watch::channel(Some(connection.clone()));
        let state_machine = RabbitmqConnectionStateMachine::new(
            config.clone(),
            connection,
            connection_tx,
            open_connection_args,
            callback,
            blocked_tx,
        );

        let keep_alive_handle = tokio::spawn(keep_alive(Arc::clone(&close_notify), state_machine));

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
    pub async fn close(self) -> anyhow::Result<()> {
        let Ok(inner) = Arc::try_unwrap(self.inner) else {
            anyhow::bail!("closing connection when connection clones exist is forbidden");
        };

        tracing::info!("closing keep alive task");
        inner.close_notify.notify_one();
        inner.keep_alive_handle.await.unwrap(); // task can't be aborted and will never panic
        tracing::info!("closed keep alive task");

        tracing::info!("closing connection");
        match inner.connection_rx.borrow().clone() {
            Some(connection) => match connection.close().await {
                Ok(()) => tracing::info!("connection closed"),
                Err(err) => tracing::warn!(%err, "closing connection failed"),
            },
            None => tracing::info!("connection already closed"),
        }

        Ok(())
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

#[tracing::instrument(
    name = "RabbitMQ Connection",
    target = "rabbitmq_client::connection",
    skip_all
)]
async fn keep_alive(close_notify: Arc<Notify>, mut state_machine: RabbitmqConnectionStateMachine) {
    tracing::info!("keep alive started");

    tokio::select! {
        biased;

        _ = close_notify.notified() => {}
        _ = state_machine.run() => {}
    }

    tracing::info!("keep alive finished");
}
