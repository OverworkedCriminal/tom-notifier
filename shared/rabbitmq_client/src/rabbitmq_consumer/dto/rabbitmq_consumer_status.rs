#[derive(PartialEq, Eq)]
pub enum RabbitmqConsumerStatus {
    Consuming,
    Recovering,
}
