// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::precision::{self, Clock, Timestamp};
use core::task::Poll;
use std::{fmt::Debug, future::poll_fn};

#[derive(Clone, Debug)]
pub struct Timer<C: Clock> {
    clock: C,
    target: Option<Timestamp>,
    armed: bool,
}

impl<C: precision::Clock> Timer<C> {
    pub fn new(clock: C) -> Self {
        Self {
            clock,
            target: None,
            armed: false,
        }
    }
}

impl<C: precision::Clock + Send + Sync + Clone> precision::Clock for Timer<C> {
    type Timer = Self;

    fn now(&self) -> Timestamp {
        self.clock.now()
    }

    fn timer(&self) -> Self::Timer {
        self.clone()
    }
}

impl<C: precision::Clock + Send + Sync + Clone> precision::Timer for Timer<C> {
    fn now(&self) -> Timestamp {
        precision::Clock::now(self)
    }

    async fn sleep_until(&mut self, target: Timestamp) {
        self.update(target);
        poll_fn(|cx| self.poll_ready(cx)).await
    }

    fn poll_ready(&mut self, _cx: &mut core::task::Context) -> Poll<()> {
        if !self.armed {
            return Poll::Ready(());
        }

        if let Some(target) = self.target {
            if self.clock.now() >= target {
                self.cancel();
                Poll::Ready(())
            } else {
                // We don't use the waker in busy poll since all futures are polled all the time
                Poll::Pending
            }
        } else {
            Poll::Ready(())
        }
    }

    fn update(&mut self, target: Timestamp) {
        self.target = Some(target);
        self.armed = true;
    }

    fn cancel(&mut self) {
        self.armed = false;
        self.target = None;
    }

    fn is_armed(&self) -> bool {
        self.armed
    }
}

impl<C: precision::Clock> s2n_quic_core::time::Clock for Timer<C> {
    #[inline]
    fn get_time(&self) -> s2n_quic_core::time::Timestamp {
        self.clock.now().into()
    }
}
