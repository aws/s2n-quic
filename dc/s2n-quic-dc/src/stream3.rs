// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3: Frame-aggregated pipeline streams
//!
//! Built on the same reliable datagram infrastructure as stream2, but with a fundamentally
//! different transport layer. Instead of one-packet-per-frame, the Peer Context aggregates
//! multiple Frames from different streams into single encrypted packets, amortizing per-packet
//! costs (encryption, packet number allocation, ACK processing) across many application writes.
//!
//! The application interface (Writer, Reader, Client, Server) is largely the same as stream2.
//! The architectural difference is entirely below the application layer.

pub mod client;
pub mod endpoint;
pub mod frame;
mod reader;
pub mod server;
mod stream;
mod writer;

pub use client::Client;
pub use reader::Reader;
pub use server::Server;
pub use stream::Stream;
pub use writer::Writer;
