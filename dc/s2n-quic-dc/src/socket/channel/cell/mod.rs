// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Single-value cell channels with capacity 1.
//!
//! These channels block on send when the cell is full, and block on receive when empty.

/// Non-Send SPSC channel with capacity 1, backed by `UnsafeCell`.
///
/// This implementation assumes futures are busy polled. As such, wakers are not used at all.
pub mod unsync;

/// Send-safe SPSC channel with capacity 1, backed by `Mutex`+`Waker`.
///
/// For use with normal async runtimes (tokio, bach, etc.) where the sender
/// and receiver may live on different threads/tasks.
pub mod sync;
