use crate::repository::entity::NotificationFindEntity;
use bson::{oid::ObjectId, Uuid};
use time::OffsetDateTime;

pub struct Notification {
    pub id: ObjectId,
    pub created_at: OffsetDateTime,
    pub producer_id: Uuid,
    pub seen: bool,
    pub content_type: String,
    pub content: Vec<u8>,
}

impl From<NotificationFindEntity> for Notification {
    fn from(entity: NotificationFindEntity) -> Self {
        Self {
            id: entity._id,
            created_at: OffsetDateTime::from(entity.created_at),
            producer_id: entity.producer_id,
            seen: entity
                .confirmations
                .first()
                .map(|confirmation| confirmation.notification_seen)
                .unwrap_or(false),
            content_type: entity.content_type,
            content: entity.content.bytes,
        }
    }
}
