use super::NotificationFindEntity;
use bson::oid::ObjectId;
use time::OffsetDateTime;

pub struct Notification {
    pub id: ObjectId,
    pub created_at: OffsetDateTime,
    pub seen: bool,
    pub content_type: String,
    pub content: Vec<u8>,
}

impl From<NotificationFindEntity> for Notification {
    fn from(entity: NotificationFindEntity) -> Self {
        Self {
            id: entity._id,
            created_at: OffsetDateTime::from(entity.created_at),
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
