// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    units::{Byte, Rate},
    Result,
};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use futures::ready;
use tokio::time::{sleep_until, Instant, Sleep};

#[derive(Debug, Default)]
pub struct Timer {
    sleep: Option<Pin<Box<Sleep>>>,
    is_armed: bool,
    window: Byte,
}

impl Timer {
    pub fn poll(&mut self, cx: &mut Context) -> Poll<()> {
        if !self.is_armed {
            return Poll::Ready(());
        }

        if let Some(timer) = self.sleep.as_mut() {
            ready!(timer.as_mut().poll(cx));
            self.is_armed = false;
        }

        Poll::Ready(())
    }

    pub fn sleep(&mut self, duration: Duration) {
        let deadline = Instant::now() + duration;
        if let Some(timer) = self.sleep.as_mut() {
            Sleep::reset(timer.as_mut(), deadline);
        } else {
            self.sleep = Some(Box::pin(sleep_until(deadline)));
        }
        self.is_armed = true;
    }

    pub fn transfer<F: FnMut(Byte, &mut Context) -> Poll<Result<u64>>>(
        &mut self,
        remaining: &mut Byte,
        rate: &Option<Rate>,
        cx: &mut Context,
        mut f: F,
    ) -> Poll<Result<()>> {
        loop {
            let amount = if let Some(Rate { bytes, period }) = rate.as_ref() {
                if self.poll(cx).is_ready() {
                    self.window = *bytes.min(remaining);
                    self.sleep(*period);
                }

                if self.window == Byte::default() {
                    return Poll::Pending;
                }

                let amount = ready!(f(self.window, cx))?;

                self.window -= amount;

                amount
            } else {
                ready!(f(*remaining, cx))?
            };

            // if the transfer returned 0 bytes that means it's done
            if amount == 0 {
                *remaining = Byte::default();
            } else {
                *remaining -= amount;
            }

            if *remaining == Byte::default() {
                self.window = Byte::default();
                return Poll::Ready(Ok(()));
            }
        }
    }
}
