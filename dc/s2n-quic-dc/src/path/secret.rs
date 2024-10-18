// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod key;
pub mod map;
#[doc(hidden)]
pub mod receiver;
#[doc(hidden)]
pub mod schedule;
mod sender;
pub mod stateless_reset;

pub use key::{open, seal};
pub use map::Map;

/// The handshake operation may return immediately if state for the target is already cached,
/// or perform an actual handshake if not.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HandshakeKind {
    /// Handshake was skipped because a secret was already present in the cache
    Cached,
    /// Handshake was performed to generate a new secret
    Fresh,
}
