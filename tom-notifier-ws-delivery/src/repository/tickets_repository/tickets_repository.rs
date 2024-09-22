use super::Ticket;
use crate::repository;
use axum::async_trait;
use bson::oid::ObjectId;
use time::OffsetDateTime;
use uuid::Uuid;

#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait TicketsRepository: Send + Sync {
    async fn insert(
        &self,
        ticket: &str,
        user_id: Uuid,
        issued_at: OffsetDateTime,
        expire_at: OffsetDateTime,
    ) -> Result<ObjectId, repository::Error>;

    async fn find(&self, ticket: &str) -> Result<Option<Ticket>, repository::Error>;

    async fn update_used_at(
        &self,
        id: ObjectId,
        used_at: OffsetDateTime,
    ) -> Result<(), repository::Error>;
}
