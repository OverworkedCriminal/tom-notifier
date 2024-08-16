use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct NotificationFilters {
    pub seen: Option<bool>,
}
