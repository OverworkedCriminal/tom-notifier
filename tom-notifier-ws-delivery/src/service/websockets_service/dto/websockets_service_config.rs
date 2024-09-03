use std::time::Duration;

pub struct WebSocketsServiceConfig {
    pub ping_interval: Duration,

    pub retry_max_count: u8,
    pub retry_interval: Duration,
}
