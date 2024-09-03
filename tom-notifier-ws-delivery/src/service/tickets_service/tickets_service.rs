use crate::{
    dto::{input, output},
    error::Error,
    repository::Ticket,
};
use axum::async_trait;
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TicketsSerivce: Send + Sync {
    async fn create_ticket(&self, user_id: Uuid) -> Result<output::WebSocketTicket, Error>;

    async fn consume_ticket(&self, ticket: input::WebSocketTicket) -> Result<Ticket, Error>;
}
