// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! This module contains all main runtime components for receiving and sending
//! data via the QUIC protocol.

#![deny(unused_must_use)]

extern crate alloc;

mod buffer;
mod contexts;
mod interval_set;
mod processed_packet;
mod space;
mod sync;
mod transmission;
mod wakeup_queue;

pub mod connection;
pub mod endpoint;
pub mod path;
pub mod recovery;
pub mod stream;
