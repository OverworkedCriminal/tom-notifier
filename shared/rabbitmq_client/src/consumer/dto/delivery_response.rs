use amqprs::AmqpDeliveryTag;

pub enum DeliveryResponse {
    Ack {
        delivery_tag: AmqpDeliveryTag,
    },

    Nack {
        delivery_tag: AmqpDeliveryTag,
        requeue: bool,
    },
}
