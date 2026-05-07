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

// mod endpoint;
// mod reader;
mod writer;
// mod flow_control;

pub mod spawner;

// pub use endpoint::Endpoint;
// pub use reader::Reader;
pub use spawner::{LocalSpawner, Spawner};
pub use writer::Writer;
