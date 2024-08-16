use bson::{Binary, DateTime, Uuid};
use serde::Serialize;

#[derive(Serialize)]
pub struct NotificationInsertEntity {
    pub created_at: DateTime,
    pub invalidate_at: Option<DateTime>,
    pub user_ids: Vec<Uuid>,
    pub producer_id: Uuid,
    pub producer_notification_id: i64,
    pub content_type: String,
    pub content: Binary,
    pub confirmations: [(); 0],
}
