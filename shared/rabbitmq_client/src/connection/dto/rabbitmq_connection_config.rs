use std::time::Duration;

#[derive(Clone)]
pub struct RabbitmqConnectionConfig {
    pub retry_interval: Duration,
}
