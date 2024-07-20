use bson::oid::ObjectId;
use time::OffsetDateTime;

pub struct Notification {
    pub id: ObjectId,
    pub created_at: OffsetDateTime,
    pub seen: bool,
    pub content_type: String,
    pub content: Vec<u8>,
}
