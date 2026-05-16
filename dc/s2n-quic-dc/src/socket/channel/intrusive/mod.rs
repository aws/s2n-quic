// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Specialized channel implementations for intrusive queues.
//!
//! These channels have no backpressure on sends since entries are pre-allocated.
//! The sender can always push to the queue, and the receiver drains until empty.

pub mod datagram_completion;
pub mod sharded;
pub mod sync;
pub mod unsync;
