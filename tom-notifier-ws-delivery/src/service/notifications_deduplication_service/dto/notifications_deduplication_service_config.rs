use std::time::Duration;

pub struct NotificationsDeduplicationServiceConfig {
    pub notification_lifespan: Duration,
    pub garbage_collector_interval: Duration,
}
