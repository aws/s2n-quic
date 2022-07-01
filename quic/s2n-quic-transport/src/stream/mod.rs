// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains the implementation of QUIC `Streams` and their management

mod api;
mod controller;
mod incoming_connection_flow_controller;
mod manager;
mod outgoing_connection_flow_controller;
mod receive_stream;
mod send_stream;
mod stream_container;
mod stream_events;
mod stream_impl;
mod stream_interests;

#[cfg(debug_assertions)]
pub(crate) mod contract;

pub use api::*;
pub use controller::Controller;
pub use manager::AbstractStreamManager;
pub use s2n_quic_core::stream::limits::Limits;
pub use stream_events::StreamEvents;
pub use stream_impl::{StreamImpl, StreamTrait};

pub type StreamManager = AbstractStreamManager<StreamImpl>;

// Import all tests

#[cfg(test)]
mod tests;
