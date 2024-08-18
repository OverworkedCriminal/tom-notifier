use crate::repository::tickets_repository::entity::TicketFindEntity;
use bson::oid::ObjectId;
use time::OffsetDateTime;
use uuid::Uuid;

pub struct Ticket {
    pub _id: ObjectId,

    pub ticket: String,

    pub user_id: Uuid,

    pub issued_at: OffsetDateTime,
    pub expire_at: OffsetDateTime,
    pub used_at: Option<OffsetDateTime>,
}

impl From<TicketFindEntity> for Ticket {
    fn from(value: TicketFindEntity) -> Self {
        Self {
            _id: value._id,
            ticket: value.ticket,
            user_id: value.user_id.into(),
            issued_at: value.issued_at.into(),
            expire_at: value.expire_at.into(),
            used_at: value.used_at.map(OffsetDateTime::from),
        }
    }
}
