use bson::{oid::ObjectId, Binary, DateTime, Uuid};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct NotificationFindEntity {
    pub _id: ObjectId,
    pub created_at: DateTime,
    pub producer_id: Uuid,
    pub content_type: String,
    pub content: Binary,

    #[serde(default)]
    pub confirmations: Vec<NotificationConfirmationFindEntity>,
}

#[derive(Deserialize)]
pub struct NotificationConfirmationFindEntity {
    pub notification_seen: bool,
}
