use super::ApplicationStateToClose;
use std::sync::Arc;

pub async fn close(state: ApplicationStateToClose) {
    tracing::info!("closing rabbitmq notifications consumer service");
    state.rabbitmq_consumer_service.close().await;
    match Arc::try_unwrap(state.deduplication_service) {
        Ok(deduplication_service) => deduplication_service.close().await,
        Err(_) => tracing::error!("cannot close deduplication service"),
    }

    tracing::info!("closing rabbimq confirmations service");
    match Arc::try_unwrap(state.rabbitmq_confirmations_service) {
        Ok(confirmations_service) => confirmations_service.close().await,
        Err(_) => tracing::error!("cannot close rabbitmq confirmations service"),
    }

    tracing::info!("closing rabbitmq connection");
    state.rabbitmq_connection.close().await;

    tracing::info!("closing connection with database");
    state.db_client.shutdown().await;
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("starting shutdown");
}
