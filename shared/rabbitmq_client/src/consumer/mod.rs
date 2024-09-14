//!
//! Module with tools that allow to consume messages from RabbitMQ queues
//!

pub mod callback;
pub mod error;

mod async_consumer;
mod channel_callback;
mod dto;
mod rabbitmq_consumer;
mod state_machine;

pub use dto::RabbitmqConsumerStatus;
pub use rabbitmq_consumer::*;
