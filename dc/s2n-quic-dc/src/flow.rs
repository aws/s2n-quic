// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Flow-based routing and queue management.
//!
//! This module provides infrastructure for managing flows with queue-based
//! routing of decrypted application data, as an alternative to the full
//! stream infrastructure for datagram-based protocols.

mod handle;

pub mod queue;

pub use handle::{Handle, Request, Tracker};
