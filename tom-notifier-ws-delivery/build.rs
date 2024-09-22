//!
//! Generate Protobuf code
//!

use std::path::PathBuf;

fn main() {
    let protobuf_path = ["..", "shared", "protobuf"]
        .into_iter()
        .collect::<PathBuf>();

    prost_build::Config::new()
        .include_file("protobuf.rs")
        .compile_protos(
            &[
                protobuf_path.join("notification.proto"),
                protobuf_path.join("rabbitmq_confirmation.proto"),
                protobuf_path.join("rabbitmq_notification.proto"),
                protobuf_path.join("websocket_notification.proto"),
                protobuf_path.join("websocket_confirmation.proto"),
            ],
            &[protobuf_path],
        )
        .unwrap();
}
