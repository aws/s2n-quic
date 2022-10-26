// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Asserts that a boolean expression is true at runtime, only if debug_assertions are enabled.
///
/// Otherwise, the compiler is told to assume that the expression is always true and can perform
/// additional optimizations.
macro_rules! unsafe_assert {
    ($cond:expr) => {
        unsafe_assert!($cond, "assumption failed: {}", stringify!($cond));
    };
    ($cond:expr $(, $fmtarg:expr)* $(,)?) => {
        let v = $cond;

        debug_assert!(v $(, $fmtarg)*);
        if cfg!(not(debug_assertions)) && !v {
            core::hint::unreachable_unchecked();
        }
    };
}

mod recv;
mod send;
mod slice;
mod state;

use slice::*;
use state::*;

macro_rules! impl_channel {
    ($name:ident) => {
        pub use recv::$name::*;
        pub use send::$name::*;

        #[inline]
        pub fn channel<T>(capacity: usize) -> (send::$name::Sender<T>, recv::$name::Receiver<T>) {
            let state = State::new(capacity);
            let sender = send::$name(state.clone());
            let receiver = recv::$name(state);
            (sender, receiver)
        }
    };
}

pub mod contiguous {
    use super::*;
    impl_channel!(double_ring);
}

pub mod generic {
    use super::*;
    pub use recv::{Receiver, RecvSlice};
    pub use send::{SendSlice, Sender};
    pub use state::{Behavior, DoubleRing, Ring};

    #[inline]
    pub fn channel<T, B: Behavior>(capacity: usize) -> (Sender<T, B>, Receiver<T, B>) {
        let state = State::new(capacity);
        let sender = Sender(state.clone());
        let receiver = Receiver(state);
        (sender, receiver)
    }
}

impl_channel!(ring);

#[cfg(test)]
mod tests;

type Result<T, E = ClosedError> = core::result::Result<T, E>;

#[derive(Clone, Copy, Debug)]
pub struct ClosedError;

#[derive(Clone, Copy, Debug)]
pub enum PushError<T> {
    Full(T),
    Closed,
}

impl<T> From<ClosedError> for PushError<T> {
    fn from(_error: ClosedError) -> Self {
        Self::Closed
    }
}
