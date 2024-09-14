#[derive(Debug, PartialEq, Eq)]
pub enum RabbitmqConsumerStatus {
    Consuming,
    Recovering,
}
