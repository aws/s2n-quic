// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

mod recv;
mod send;
mod slice;
mod state;

use slice::*;
use state::*;

pub use recv::{Receiver, RecvSlice};
pub use send::{SendSlice, Sender};

#[inline]
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    let state = State::new(capacity);
    let sender = Sender(state.clone());
    let receiver = Receiver(state);
    (sender, receiver)
}

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
