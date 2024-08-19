//!
//! Generate Protobuf code
//!

fn main() {
    prost_build::Config::new()
        .include_file("protobuf.rs")
        .compile_protos(
            &[
                "../shared/protobuf/notification.proto",
                "../shared/protobuf/confirmation.proto",
                "../shared/protobuf/rabbitmq_notification.proto",
                "../shared/protobuf/websocket_notification.proto",
            ],
            &["../shared/protobuf/"],
        )
        .unwrap();
}
