use bson::oid::ObjectId;
use time::OffsetDateTime;
use uuid::Uuid;

pub struct InsertedNotification {
    pub id: ObjectId,
    pub created_at: OffsetDateTime,
    pub invalidate_at: Option<OffsetDateTime>,
    pub user_ids: Vec<Uuid>,
    pub producer_id: Uuid,
    pub producer_notification_id: i64,
    pub content_type: String,
    pub content: Vec<u8>,
}
