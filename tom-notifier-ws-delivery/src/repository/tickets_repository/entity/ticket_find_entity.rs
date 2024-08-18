use bson::{oid::ObjectId, DateTime, Uuid};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct TicketFindEntity {
    pub _id: ObjectId,

    pub ticket: String,

    pub user_id: Uuid,

    pub issued_at: DateTime,
    pub expire_at: DateTime,
    pub used_at: Option<DateTime>,
}
