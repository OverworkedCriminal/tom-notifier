use serde::Deserialize;

#[derive(Deserialize)]
pub struct NotificationFilters {
    pub seen: Option<bool>,
}
