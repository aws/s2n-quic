// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the implementation of QUIC `Streams` and their management

mod api;
mod incoming_connection_flow_controller;
mod peer_controller;
mod outgoing_connection_flow_controller;
mod local_controller;
mod receive_stream;
mod send_stream;
mod stream_container;
mod stream_events;
mod stream_impl;
mod stream_interests;
mod stream_manager;

#[cfg(debug_assertions)]
pub(crate) mod contract;

pub use api::*;
pub use s2n_quic_core::stream::limits::Limits;
pub use stream_events::StreamEvents;
pub use stream_impl::{StreamImpl, StreamTrait};
pub use stream_manager::AbstractStreamManager;
pub use peer_controller::PeerController;
pub use local_controller::LocalController;

pub type StreamManager = AbstractStreamManager<StreamImpl>;

// Import all tests

#[cfg(test)]
mod tests;
