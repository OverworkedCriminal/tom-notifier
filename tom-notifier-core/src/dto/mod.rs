//!
//! Module with all dtos that are passed between server and users
//!

pub mod input;
pub mod output;

pub mod protobuf {
    include!(concat!(env!("OUT_DIR"), "/protobuf.rs"));
}
