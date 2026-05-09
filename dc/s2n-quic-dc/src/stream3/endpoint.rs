// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3 Endpoint: shared infrastructure for the process.

pub(crate) mod ack;
pub(crate) mod assemble;
pub(crate) mod counters;
pub(crate) mod dispatch;
pub(crate) mod inflight;
pub(crate) mod msg;
pub(crate) mod recv;
pub(crate) mod reset_error;
pub(crate) mod routing;
pub(crate) mod send;
pub(crate) mod socket;
pub(crate) mod worker;

use crate::{
    acceptor,
    stream3::{frame::{Frame, SubmissionSender}, Stream},
};
use std::sync::atomic::AtomicU64;

pub struct Endpoint {
    /// Frame submission channel (writers submit Queue<Frame> here)
    pub frame_tx: SubmissionSender,
    /// Path secret map (shared with PSK providers)
    pub path_secret_map: crate::path::secret::Map,
    /// Queue allocator for flow queues
    pub queue_allocator: msg::queue::Allocator,
    /// Acceptor registry for server-side stream dispatch
    pub acceptor_registry: acceptor::Registry<Stream>,
    /// Endpoint-wide stream ID counter
    pub next_stream_id: AtomicU64,
    /// The port that recv sockets are bound to
    pub data_port: u16,
}
