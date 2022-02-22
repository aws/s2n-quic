// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::units::{Byte, ByteExt, Rate};
use core::{
    task::{Context, Poll},
    time::Duration,
};
use futures::ready;
pub use s2n_quic_core::time::{
    timer::{self, Provider, Query, Result},
    Timestamp,
};

#[derive(Debug, Default)]
pub struct Timer {
    sleep: s2n_quic_core::time::Timer,
    window: Byte,
}

impl timer::Provider for Timer {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.sleep.timers(query)
    }
}

impl Timer {
    pub fn poll(&mut self, now: Timestamp) -> Poll<()> {
        if !self.sleep.is_armed() {
            Poll::Ready(())
        } else {
            self.sleep.poll_expiration(now)
        }
    }

    pub fn sleep(&mut self, now: Timestamp, duration: Duration) {
        self.sleep.set(now + duration);
    }

    pub fn transfer<F: FnMut(Byte, &mut Context) -> Poll<crate::Result<u64>>>(
        &mut self,
        remaining: &mut Byte,
        rate: &Option<Rate>,
        now: Timestamp,
        cx: &mut Context,
        mut f: F,
    ) -> Poll<crate::Result<()>> {
        loop {
            let amount = if let Some(Rate { bytes, period }) = rate.as_ref() {
                if self.poll(now).is_ready() {
                    self.window = *bytes.min(remaining);
                    self.sleep(now, *period);
                }

                if self.window == Byte::default() {
                    return Poll::Pending;
                }

                let amount = ready!(f(self.window, cx))?.bytes();

                self.window -= amount;

                amount
            } else {
                ready!(f(*remaining, cx))?.bytes()
            };

            // if the transfer returned 0 bytes that means it's done
            if amount == 0.bytes() {
                *remaining = Byte::default();
            } else {
                *remaining -= amount.bytes();
            }

            if *remaining == Byte::default() {
                self.window = Byte::default();
                return Poll::Ready(Ok(()));
            }
        }
    }
}
