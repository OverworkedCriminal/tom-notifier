use bson::{DateTime, Uuid};
use serde::Serialize;

#[derive(Serialize)]
pub struct TicketInsertEntity<'a> {
    pub ticket: &'a str,

    pub user_id: Uuid,

    pub issued_at: DateTime,
    pub expire_at: DateTime,
    pub used_at: Option<DateTime>,
}
