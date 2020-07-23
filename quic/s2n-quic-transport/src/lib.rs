//! This module contains all main runtime components for receiving and sending
//! data via the QUIC protocol.

#![deny(unused_must_use)]

extern crate alloc;

mod buffer;
mod contexts;
mod frame_exchange_interests;
mod interval_set;
mod processed_packet;
mod recovery;
mod space;
mod sync;
mod timer;
mod unbounded_channel;
mod wakeup_queue;

pub mod acceptor;
pub mod connection;
pub mod endpoint;
pub mod stream;

///////////////// From here on everything is temporary

#[doc(hidden)]
pub use stream::StreamManager; // To reduce compiler warnings
