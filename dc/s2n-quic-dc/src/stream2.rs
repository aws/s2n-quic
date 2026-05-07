// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream2: Pipeline-based streams
//!
//! This module provides a stream abstraction built on top of the reliable datagram pipeline.
//! Unlike the original streams in the stream/ directory which implement packet-level reliability,
//! stream2 delegates all reliability concerns to the pipeline and focuses solely on:
//!
//! - Fragmentation: Breaking application data into MTU-sized datagrams
//! - Reassembly: Converting out-of-order datagrams back into an ordered byte stream
//! - Flow control: Managing local and remote flow control windows
//!
//! The pipeline handles retransmission, ACKs, congestion control, and reliable delivery.
//!
//! ## Application Interface
//!
//! - `Endpoint` (in endpoint module): Shared infrastructure for the process (wheel workers,
//!   path secret map, queue allocator). Wrapped in Arc and shared by Client and Server.
//! - `Client`: Makes outbound connections via `connect(addr, acceptor_id) -> Stream`.
//! - `Server`: Accepts inbound connections via channel or handler acceptor modes.
//! - `Stream`: Bidirectional stream with `split()` to get independent Reader/Writer halves.

pub mod client;
pub mod endpoint;
mod reader;
pub mod server;
pub mod spawner;
mod stream;
mod writer;

pub use client::Client;
pub use reader::Reader;
pub use server::Server;
pub use spawner::{LocalSpawner, Spawner};
pub use stream::Stream;
pub use writer::Writer;
