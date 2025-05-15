// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::{Context, Poll};
use s2n_quic_core::time::{Clock, Timestamp};
use tracing::trace;

#[derive(Default)]
pub struct Naive {
    transmissions_without_yield: u8,
    yield_window: Option<Timestamp>,
}

impl Naive {
    #[inline]
    pub fn poll_pacing<C: Clock + ?Sized>(&mut self, cx: &mut Context, clock: &C) -> Poll<()> {
        if self.transmissions_without_yield < 5 {
            trace!("pass");
            self.transmissions_without_yield += 1;
            return Poll::Ready(());
        }

        // reset the counter
        self.transmissions_without_yield = 0;

        // record the time that we yielded
        let now = clock.get_time();
        let prev_yield_window = self
            .yield_window
            .replace(now + core::time::Duration::from_millis(1));

        // if the current time falls outside of the previous window then don't actually yield - the
        // application isn't sending at that rate
        if let Some(yield_window) = prev_yield_window {
            if now > yield_window {
                trace!("underflow");
                self.transmissions_without_yield += 1;
                return Poll::Ready(());
            }
        }

        trace!("yield");
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
