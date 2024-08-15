use axum::async_trait;
use bson::oid::ObjectId;
use time::OffsetDateTime;
use uuid::Uuid;

///
/// Service used to propagate notification state change to any intrested party
///
#[cfg_attr(test, mockall::automock)]
#[async_trait]
pub trait FanoutService: Send + Sync {
    async fn send_new(
        &self,
        user_ids: Vec<Uuid>,
        id: ObjectId,
        timestamp: OffsetDateTime,
        seen: bool,
        content_type: String,
        content: Vec<u8>,
    );

    async fn send_updated(
        &self,
        user_id: Uuid,
        id: ObjectId,
        seen: bool,
        timestamp: OffsetDateTime,
    );

    async fn send_deleted(&self, user_id: Uuid, id: ObjectId, timestamp: OffsetDateTime);
}
