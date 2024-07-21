use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct NotificationSeen {
    pub seen: bool,
}
