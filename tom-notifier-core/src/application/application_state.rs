use super::ApplicationEnv;
use crate::repository::NotificationsRepositoryImpl;
use mongodb::{options::ClientOptions, Client};

#[derive(Clone)]
pub struct ApplicationState {}

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

    Ok((ApplicationState {}, ApplicationStateToClose { db_client }))
}
