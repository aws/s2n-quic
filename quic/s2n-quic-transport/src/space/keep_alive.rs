// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{task::Poll, time::Duration};
use s2n_quic_core::time::{timer, Timer, Timestamp};

#[derive(Debug)]
pub struct KeepAlive {
    enabled: bool,
    period: Duration,
    timer: Timer,
}

impl KeepAlive {
    pub fn new(max_idle_timeout: Option<Duration>, default_period: Duration) -> Self {
        let period = if let Some(max_idle_timeout) = max_idle_timeout {
            // send a ping frame at 3/4 max idle timeout to ensure it is delivered in time
            max_idle_timeout * 3 / 4
        } else {
            default_period
        };

        Self {
            enabled: false,
            period,
            timer: Timer::default(),
        }
    }

    #[inline]
    pub fn update(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    #[inline]
    pub fn reset(&mut self, now: Timestamp) {
        self.timer.set(now + self.period)
    }

    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) -> Poll<()> {
        if !self.enabled {
            return Poll::Pending;
        }

        let res = self.timer.poll_expiration(now);

        if res.is_ready() {
            self.reset(now);
        }

        res
    }

    #[inline]
    pub fn period(&self) -> Duration {
        self.period
    }
}

impl timer::Provider for KeepAlive {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        if self.enabled {
            self.timer.timers(query)?;
        }
        Ok(())
    }
}
