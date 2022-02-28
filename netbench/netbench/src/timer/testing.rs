// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Timestamp;
use core::{
    task::{Context, Poll, Waker},
    time::Duration,
};

#[derive(Debug)]
pub struct Timer {
    now: Timestamp,
    target: Option<Timestamp>,
    waker: Option<Waker>,
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            now: unsafe { Timestamp::from_duration(Duration::ZERO) },
            target: None,
            waker: None,
        }
    }
}

impl Timer {
    pub fn advance_pair(&mut self, other: &mut Self) -> Option<Timestamp> {
        let target = match (self.target, other.target) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, None) => a,
            (None, b) => b,
        };

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

impl super::Timer for Timer {
    fn now(&self) -> Timestamp {
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
