use amqprs::BasicProperties;

pub struct Message {
    pub routing_key: String,
    pub basic_properties: BasicProperties,
    pub content: Vec<u8>,
}
