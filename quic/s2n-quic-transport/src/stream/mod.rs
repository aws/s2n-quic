//! This module contains the implementation of QUIC `Streams` and their management

mod api;
mod error;
mod incoming_connection_flow_controller;
mod limits;
mod outgoing_connection_flow_controller;
mod receive_stream;
mod send_stream;
mod stream_container;
mod stream_events;
mod stream_impl;
mod stream_interests;
mod stream_manager;

pub use api::*;
pub use error::StreamError;
pub use limits::StreamLimits;
pub use stream_events::StreamEvents;
pub use stream_impl::{StreamImpl, StreamTrait};
pub use stream_manager::{AbstractStreamManager, StreamManagerInterests};

pub type StreamManager = AbstractStreamManager<StreamImpl>;

// Import all tests

#[cfg(test)]
mod tests;
