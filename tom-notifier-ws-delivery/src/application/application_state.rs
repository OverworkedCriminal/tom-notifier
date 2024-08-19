use super::ApplicationEnv;
use crate::{
    repository::TicketsRepositoryImpl,
    service::{
        tickets_service::{TicketsSerivce, TicketsServiceConfig, TicketsServiceImpl},
        websockets_service::{WebSocketsService, WebSocketsServiceImpl},
    },
};
use axum::extract::FromRef;
use mongodb::{options::ClientOptions, Client};
use std::sync::Arc;

#[derive(Clone, FromRef)]
pub struct ApplicationState {
    pub tickets_service: Arc<dyn TicketsSerivce>,
    pub websockets_service: Arc<dyn WebSocketsService>,
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
    let tickets_repository = TicketsRepositoryImpl::new(db).await?;
    let tickets_repository = Arc::new(tickets_repository);

    tracing::info!("creating services");
    let config = TicketsServiceConfig {
        ticket_lifespan: env.websocket_ticket_lifespan,
    };
    let tickets_service = TicketsServiceImpl::new(config, tickets_repository);
    let tickets_service = Arc::new(tickets_service);

    let websockets_service = WebSocketsServiceImpl::new();
    let websockets_service = Arc::new(websockets_service); 

    Ok((
        ApplicationState {
            tickets_service,
            websockets_service,
        },
        ApplicationStateToClose { db_client },
    ))
}
