// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[macro_use]
pub mod tracing;

pub mod acceptor;
pub mod allocator;
pub mod bitset;
pub mod busy_poll;
pub mod byte_vec;
pub mod congestion;
pub mod control;
pub mod counter;
pub mod credentials;
pub mod credit;
pub mod crypto;
pub mod datagram;
pub mod endpoint;
pub mod event;
pub mod intrusive;
pub mod msg;
pub mod packet;
pub mod path;
pub mod psk;
pub mod queue;
pub mod recovery;
pub mod runtime;
pub mod socket;
pub mod stream;
pub mod sync;
pub mod task;
pub mod time;
pub mod uds;
pub mod xorshift;

#[deprecated = "use stream instead of stream3"]
pub use stream as stream3;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use s2n_quic_core::dc::{Version, SUPPORTED_VERSIONS};
