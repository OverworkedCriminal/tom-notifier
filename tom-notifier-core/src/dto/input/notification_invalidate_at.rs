use serde::Deserialize;
use time::OffsetDateTime;

#[derive(Debug, Deserialize)]
pub struct NotificationInvalidateAt {
    pub invalidate_at: Option<OffsetDateTime>,
}
