// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};
pub use s2n_quic_core::time::Timestamp;

pub trait Timer {
    fn now(&self) -> Timestamp;
    fn poll(&mut self, target: Timestamp, cx: &mut Context) -> Poll<()>;
}

#[derive(Debug)]
pub struct Testing {
    now: Timestamp,
    target: Option<Timestamp>,
    waker: Option<Waker>,
}

impl Default for Testing {
    fn default() -> Self {
        Self {
            now: unsafe { Timestamp::from_duration(Duration::ZERO) },
            target: None,
            waker: None,
        }
    }
}

impl Testing {
    pub fn advance_pair(&mut self, other: &mut Self) -> Option<Timestamp> {
        let target = self.target.map(|target| {
            if let Some(other) = other.target {
                target.min(other)
            } else {
                target
            }
        });

        if let Some(target) = target {
            self.set(target);
            other.set(target);
            Some(self.now)
        } else {
            None
        }
    }

    fn set(&mut self, target: Timestamp) {
        self.now = target;
        self.target = None;
        if let Some(waker) = self.waker.take() {
            waker.wake();
        }
    }
}

impl Timer for Testing {
    fn now(&self) -> s2n_quic_core::time::Timestamp {
        self.now
    }

    fn poll(&mut self, target: Timestamp, cx: &mut Context<'_>) -> Poll<()> {
        if target == self.now {
            Poll::Ready(())
        } else {
            self.target = Some(target);
            self.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
