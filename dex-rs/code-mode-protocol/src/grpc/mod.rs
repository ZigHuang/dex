#[cfg(dex_bazel)]
pub use code_mode_proto::dex::code_mode::v1::*;

#[cfg(not(dex_bazel))]
tonic::include_proto!("dex.code_mode.v1");

pub const MAX_IDENTIFIER_BYTES: usize = 256;
