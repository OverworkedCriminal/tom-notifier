use super::ApplicationEnv;
use crate::{
    repository::NotificationsRepositoryImpl,
    service::{
        confirmations_consumer_service::{
            ConfirmationsConsumerService, ConfirmationsConsumerServiceConfig,
        },
        notifications_producer_service::{
            NotificationsProducerServiceConfig, NotificationsProducerServiceImpl,
        },
        notifications_service::{
            NotificationsService, NotificationsServiceConfig, NotificationsServiceImpl,
        },
    },
};
use amqprs::connection::OpenConnectionArguments;
use axum::extract::FromRef;
use mongodb::{options::ClientOptions, Client};
use rabbitmq_client::connection::{RabbitmqConnection, RabbitmqConnectionConfig};
use std::sync::Arc;

#[derive(Clone, FromRef)]
pub struct ApplicationState {
    pub notifications_service: Arc<dyn NotificationsService>,
}

pub struct ApplicationStateToClose {
    pub db_client: Client,
    pub rabbitmq_connection: RabbitmqConnection,
    pub rabbitmq_notifications_producer_service: Arc<NotificationsProducerServiceImpl>,
    pub rabbitmq_confirmations_consumer_service: ConfirmationsConsumerService,
}

pub async fn create_state(
    env: &ApplicationEnv,
) -> anyhow::Result<(ApplicationState, ApplicationStateToClose)> {
    tracing::info!("connecting to database");
    let db_client_options = ClientOptions::parse(&env.db_connection_string).await?;
    let db_client = Client::with_options(db_client_options)?;
    let db = db_client.database(&env.db_name);

    tracing::info!("creating repositories");
    let notifications_repository = NotificationsRepositoryImpl::new(db).await?;
    let notifications_repository = Arc::new(notifications_repository);

    tracing::info!("creating services");
    let config = RabbitmqConnectionConfig {
        retry_interval: env.rabbitmq_retry_interval,
    };
    let open_connection_args =
        OpenConnectionArguments::try_from(env.rabbitmq_connection_string.as_str())?;
    let rabbitmq_connection = RabbitmqConnection::new(config, open_connection_args).await?;

    let config = NotificationsProducerServiceConfig {
        exchange_name: env.rabbitmq_notifications_exchange_name.clone(),
    };
    let rabbitmq_notifications_producer_service =
        NotificationsProducerServiceImpl::new(config, rabbitmq_connection.clone()).await?;
    let rabbitmq_notifications_producer_service = Arc::new(rabbitmq_notifications_producer_service);

    let config = ConfirmationsConsumerServiceConfig {
        exchange: env.rabbitmq_confirmations_exchange_name.clone(),
        queue: env.rabbitmq_confirmations_queue_name.clone(),
    };
    let rabbitmq_confirmations_consumer_service = ConfirmationsConsumerService::new(
        config,
        rabbitmq_connection.clone(),
        notifications_repository.clone(),
    )
    .await?;

    let notifications_service_config = NotificationsServiceConfig {
        max_content_len: env.max_notification_content_len,
    };
    let notifications_service = NotificationsServiceImpl::new(
        notifications_service_config,
        notifications_repository,
        rabbitmq_notifications_producer_service.clone(),
    );
    let notifications_service = Arc::new(notifications_service);

    Ok((
        ApplicationState {
            notifications_service,
        },
        ApplicationStateToClose {
            db_client,
            rabbitmq_connection,
            rabbitmq_notifications_producer_service,
            rabbitmq_confirmations_consumer_service,
        },
    ))
}
