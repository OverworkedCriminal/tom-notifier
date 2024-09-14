//!
//! Module with user programmable callbacks
//!

mod rabbitmq_consumer_delivery_callback;
mod rabbitmq_consumer_status_change_callback;

pub use rabbitmq_consumer_delivery_callback::*;
pub use rabbitmq_consumer_status_change_callback::*;
