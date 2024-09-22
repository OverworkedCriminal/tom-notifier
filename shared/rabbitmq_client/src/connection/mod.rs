//!
//! Module that allows to establish connection with RabbitMQ server.
//!

mod connection_callback;
mod dto;
mod rabbitmq_connection;
mod state_machine;

pub use dto::RabbitmqConnectionConfig;
pub use rabbitmq_connection::RabbitmqConnection;
