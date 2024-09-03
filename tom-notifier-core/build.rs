//!
//! Generate Protobuf code
//!

fn main() {
    prost_build::Config::new()
        .include_file("protobuf.rs")
        .compile_protos(
            &[
                "../shared/protobuf/notification.proto",
                "../shared/protobuf/rabbitmq_confirmation.proto",
                "../shared/protobuf/rabbitmq_notification.proto",
            ],
            &["../shared/protobuf/"],
        )
        .unwrap();
}
