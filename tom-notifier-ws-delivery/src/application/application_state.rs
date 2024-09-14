use super::ApplicationEnv;
use crate::{
    repository::TicketsRepositoryImpl,
    service::{
        confirmations_service::{ConfirmationsServiceConfig, ConfirmationsServiceImpl},
        notifications_consumer_service::{
            NotificationsConsumerService, NotificationsConsumerServiceConfig,
        },
        notifications_deduplication_service::NotificationsDeduplicationServiceImpl,
        tickets_service::{TicketsSerivce, TicketsServiceConfig, TicketsServiceImpl},
        websockets_service::{WebSocketsService, WebSocketsServiceConfig, WebSocketsServiceImpl},
    },
};
use amqprs::connection::OpenConnectionArguments;
use axum::extract::FromRef;
use mongodb::{options::ClientOptions, Client};
use rabbitmq_client::{RabbitmqConnection, RabbitmqConnectionConfig};
use std::sync::Arc;

#[derive(Clone, FromRef)]
pub struct ApplicationState {
    pub tickets_service: Arc<dyn TicketsSerivce>,
    pub websockets_service: Arc<dyn WebSocketsService>,
}

pub struct ApplicationStateToClose {
    pub db_client: Client,
    pub rabbitmq_connection: RabbitmqConnection,
    pub rabbitmq_confirmations_service: Arc<ConfirmationsServiceImpl>,
    pub rabbitmq_consumer_service: NotificationsConsumerService,
}

pub async fn create_state(
    env: &ApplicationEnv,
) -> anyhow::Result<(ApplicationState, ApplicationStateToClose)> {
    tracing::info!("connecting to database");
    let db_client_options = ClientOptions::parse(&env.db_connection_string).await?;
    let db_client = Client::with_options(db_client_options)?;
    let db = db_client.database(&env.db_name);

    tracing::info!("creating repositories");
    let tickets_repository = TicketsRepositoryImpl::new(db).await?;
    let tickets_repository = Arc::new(tickets_repository);

    tracing::info!("creating services");
    let config = TicketsServiceConfig {
        ticket_lifespan: env.websocket_ticket_lifespan,
    };
    let tickets_service = TicketsServiceImpl::new(config, tickets_repository);
    let tickets_service = Arc::new(tickets_service);

    let config = RabbitmqConnectionConfig {
        retry_interval: env.rabbitmq_retry_interval,
    };
    let open_connection_args =
        OpenConnectionArguments::try_from(env.rabbitmq_connection_string.as_str())?;
    let rabbitmq_connection = RabbitmqConnection::new(config, open_connection_args).await?;

    let config = ConfirmationsServiceConfig {
        exchange: env.rabbitmq_confirmations_exchange_name.clone(),
    };
    let rabbitmq_confirmations_service =
        ConfirmationsServiceImpl::new(config, rabbitmq_connection.clone()).await?;
    let rabbitmq_confirmations_service = Arc::new(rabbitmq_confirmations_service);

    let config = WebSocketsServiceConfig {
        ping_interval: env.websocket_ping_interval,
        retry_max_count: env.websocket_retry_max_count,
        retry_interval: env.websocket_retry_interval,
        connection_buffer_size: env.websocket_connection_buffer_size,
    };
    let websockets_service =
        WebSocketsServiceImpl::new(config, rabbitmq_confirmations_service.clone());
    let websockets_service = Arc::new(websockets_service);

    let deduplication_service = NotificationsDeduplicationServiceImpl::new();
    let deduplication_service = Arc::new(deduplication_service);

    let config = NotificationsConsumerServiceConfig {
        exchange: env.rabbitmq_notifications_exchange_name.clone(),
        queue: env.rabbitmq_notifications_queue_name.clone(),
    };
    let rabbitmq_consumer_service = NotificationsConsumerService::new(
        config,
        rabbitmq_connection.clone(),
        websockets_service.clone(),
        deduplication_service,
    )
    .await?;

    Ok((
        ApplicationState {
            tickets_service,
            websockets_service,
        },
        ApplicationStateToClose {
            db_client,
            rabbitmq_connection,
            rabbitmq_confirmations_service,
            rabbitmq_consumer_service,
        },
    ))
}
