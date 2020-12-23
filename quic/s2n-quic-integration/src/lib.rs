#![recursion_limit = "256"]

pub mod api;
pub mod packet;
pub mod s2n_quic;
pub mod stream;

// TODO abstract over the current runtime
pub use tokio::spawn;
