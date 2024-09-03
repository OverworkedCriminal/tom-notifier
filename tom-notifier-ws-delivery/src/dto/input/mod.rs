pub use super::inoutput::WebSocketTicket;

#[allow(unused_imports)]
pub use super::protobuf::{
    notification::{
        NotificationProtobuf,
        // NotificationProtobuf is exported so there's no reason not to export
        // NotificationStatusProtobuf. But it is used only in tests
        // so the compiler produces warning
        NotificationStatusProtobuf,
    },
    rabbitmq_notification::RabbitmqNotificationProtobuf,
    websocket_confirmation::WebSocketConfirmationProtobuf,
};
