// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A token that wakes a stored waker on drop.
//!
//! `AutoWake` solves the "deferred wake under failure" problem: code paths that produce a wakeup
//! decision but defer the actual `wake()` (because they hold a lock, or because the waker is
//! shipped through a channel for batch delivery) need a way to guarantee the wake still runs even
//! if the deferred path fails — channel closed, batch dropped, panic on the way out.
//!
//! Wrapping the `Option<Waker>` in `AutoWake` turns "drop without delivering" into "drop with
//! wake". The producer's only obligation is to either `take()` the waker (when delivering it
//! normally) or hand the `AutoWake` to a sink that is responsible for the same; if anything in
//! between fails and the `AutoWake` drops with a still-`Some` inner waker, the parked task is
//! woken anyway. A spurious wake is recoverable; a permanently-stuck task is not.

use core::task::Waker;

/// A token that wakes a stored waker when dropped.
#[derive(Default)]
pub struct AutoWake(pub(crate) Option<Waker>);

impl AutoWake {
    pub fn new(waker: Option<Waker>) -> Self {
        Self(waker)
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    pub fn take(&mut self) -> Option<Waker> {
        self.0.take()
    }
}

impl Drop for AutoWake {
    fn drop(&mut self) {
        if let Some(w) = self.0.take() {
            w.wake();
        }
    }
}
