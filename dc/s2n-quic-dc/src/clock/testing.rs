// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Shared test clock for use with precision::Clock/Timer interfaces.
//!
//! Time is advanced via `Clock::advance()` or `Clock::set()`. All timers
//! created from the same clock observe the advancement through shared state.

use super::precision;
use core::task::Poll;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

struct Inner {
    now: precision::Timestamp,
}

#[derive(Clone)]
pub struct Clock(Arc<Mutex<Inner>>);

impl Clock {
    pub fn new(start: Duration) -> Self {
        Self(Arc::new(Mutex::new(Inner {
            now: precision::Timestamp {
                nanos: start.as_nanos() as u64,
            },
        })))
    }

    pub fn get_time(&self) -> precision::Timestamp {
        self.0.lock().unwrap().now
    }

    pub fn set(&self, time: precision::Timestamp) {
        self.0.lock().unwrap().now = time;
    }

    pub fn advance(&self, duration: Duration) {
        let mut inner = self.0.lock().unwrap();
        inner.now = inner.now + duration;
    }
}

impl precision::Clock for Clock {
    type Timer = Timer;

    fn now(&self) -> precision::Timestamp {
        self.get_time()
    }

    fn timer(&self) -> Timer {
        Timer {
            clock: self.0.clone(),
            target: None,
        }
    }
}

pub struct Timer {
    clock: Arc<Mutex<Inner>>,
    target: Option<precision::Timestamp>,
}

impl precision::Timer for Timer {
    fn now(&self) -> precision::Timestamp {
        self.clock.lock().unwrap().now
    }

    async fn sleep_until(&mut self, target: precision::Timestamp) {
        self.update(target);
        core::future::pending::<()>().await;
    }

    fn poll_ready(&mut self, _cx: &mut core::task::Context) -> Poll<()> {
        if let Some(target) = self.target {
            if self.clock.lock().unwrap().now >= target {
                self.cancel();
                return Poll::Ready(());
            }
        }
        Poll::Pending
    }

    fn update(&mut self, target: precision::Timestamp) {
        self.target = Some(target);
    }

    fn cancel(&mut self) {
        self.target = None;
    }

    fn is_armed(&self) -> bool {
        self.target.is_some()
    }
}
