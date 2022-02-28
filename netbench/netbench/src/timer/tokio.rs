// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Timestamp;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use futures::ready;
use tokio::time::{sleep, Instant, Sleep};

#[derive(Debug)]
pub struct Timer {
    start: Instant,
    target: Option<Timestamp>,
    sleep: Pin<Box<Sleep>>,
}

impl Timer {
    fn set_target(&mut self, target: Timestamp) {
        self.target = Some(target);
        let duration = unsafe { target.as_duration() };
        self.sleep.as_mut().reset(self.start + duration);
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self {
            start: Instant::now(),
            target: None,
            sleep: Box::pin(sleep(Duration::from_secs(0))),
        }
    }
}

impl super::Timer for Timer {
    fn now(&self) -> Timestamp {
        let duration = self.start.elapsed();
        unsafe { Timestamp::from_duration(duration) }
    }

    fn poll(&mut self, target: Timestamp, cx: &mut Context) -> Poll<()> {
        if let Some(prev_target) = self.target.as_mut() {
            if *prev_target != target {
                self.set_target(target);
            }
        } else {
            self.set_target(target);
        }

        ready!(self.sleep.as_mut().poll(cx));

        self.target = None;

        Poll::Ready(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timer::Timer as _;
    use futures_test::task::new_count_waker;

    #[tokio::test(start_paused = true)]
    async fn timer_test() {
        let mut timer = Timer::default();
        let (waker, _count) = new_count_waker();
        let mut cx = Context::from_waker(&waker);

        tokio::time::advance(Duration::from_secs(1)).await;

        let mut now = timer.now();

        let mut times = [now; 5];
        for (idx, time) in times.iter_mut().enumerate() {
            *time += Duration::from_secs(idx as _);
        }

        assert!(timer.poll(now, &mut cx).is_ready());

        for _ in 0..times.len() {
            // poll a bunch of different times in loop to make sure all of the branches are covered
            for time in times.iter().chain(times.iter().rev()).copied() {
                assert_eq!(timer.poll(time, &mut cx).is_ready(), time <= now);
                assert_eq!(timer.poll(time, &mut cx).is_ready(), time <= now);
            }

            tokio::time::advance(Duration::from_secs(1)).await;
            now += Duration::from_secs(1);
        }
    }
}
