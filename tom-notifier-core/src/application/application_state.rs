use super::ApplicationEnv;
use crate::{
    repository::NotificationsRepositoryImpl,
    service::notifications_service::{
        NotificationsService, NotificationsServiceConfig, NotificationsServiceImpl,
    },
};
use axum::extract::FromRef;
use mongodb::{options::ClientOptions, Client};
use std::sync::Arc;

#[derive(Clone, FromRef)]
pub struct ApplicationState {
    pub notifications_service: Arc<dyn NotificationsService>,
}

pub struct ApplicationStateToClose {
    pub db_client: Client,
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
    let notifications_service_config = NotificationsServiceConfig {
        max_content_len: env.max_notification_content_len,
    };
    let notifications_service =
        NotificationsServiceImpl::new(notifications_service_config, notifications_repository);
    let notifications_service = Arc::new(notifications_service);

    Ok((
        ApplicationState {
            notifications_service,
        },
        ApplicationStateToClose { db_client },
    ))
}
