// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use pin_project_lite::pin_project;

#[derive(Clone, Debug, Default)]
pub struct Cooldown {
    credits: u16,
    limit: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The task should loop
    Loop,
    /// The task should return Pending and wait for an actual wake notification
    Sleep,
}

impl Outcome {
    #[inline]
    pub fn is_loop(&self) -> bool {
        matches!(self, Self::Loop)
    }

    #[inline]
    pub fn is_sleep(&self) -> bool {
        matches!(self, Self::Sleep)
    }
}

impl Cooldown {
    #[inline]
    pub fn new(limit: u16) -> Self {
        Self {
            limit,
            credits: limit,
        }
    }

    #[inline]
    pub fn state(&self) -> Outcome {
        if self.credits > 0 {
            Outcome::Loop
        } else {
            Outcome::Sleep
        }
    }

    /// Notifies the cooldown that the poll operation was ready
    ///
    /// This resets the cooldown period until another `Pending` result.
    #[inline]
    pub fn on_ready(&mut self) {
        // reset the pending count
        self.credits = self.limit;
    }

    /// Notifies the cooldown that the poll operation was pending
    ///
    /// This consumes a cooldown credit until they are exhausted at which point the task should
    /// sleep.
    #[inline]
    pub fn on_pending(&mut self) -> Outcome {
        if self.credits > 0 {
            self.credits -= 1;
            return Outcome::Loop;
        }

        Outcome::Sleep
    }

    #[inline]
    pub fn on_pending_task(&mut self, cx: &mut core::task::Context) -> Outcome {
        let outcome = self.on_pending();

        if outcome.is_loop() {
            cx.waker().wake_by_ref();
        }

        outcome
    }

    #[inline]
    pub async fn wrap<F>(&mut self, fut: F) -> F::Output
    where
        F: Future + Unpin,
    {
        Wrapped {
            fut,
            cooldown: self,
        }
        .await
    }
}

pin_project!(
    struct Wrapped<'a, F>
    where
        F: core::future::Future,
    {
        #[pin]
        fut: F,
        cooldown: &'a mut Cooldown,
    }
);

impl<F> Future for Wrapped<'_, F>
where
    F: Future,
{
    type Output = F::Output;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();
        match this.fut.poll(cx) {
            Poll::Ready(v) => {
                this.cooldown.on_ready();
                Poll::Ready(v)
            }
            Poll::Pending => {
                this.cooldown.on_pending_task(cx);
                Poll::Pending
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldown_test() {
        let mut cooldown = Cooldown::new(2);

        assert_eq!(cooldown.on_pending(), Outcome::Loop);
        assert_eq!(cooldown.on_pending(), Outcome::Loop);
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);

        // call on ready to restore credits
        cooldown.on_ready();

        assert_eq!(cooldown.on_pending(), Outcome::Loop);
        assert_eq!(cooldown.on_pending(), Outcome::Loop);
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);

        cooldown.on_ready();

        // call on ready while we're still looping
        assert_eq!(cooldown.on_pending(), Outcome::Loop);
        cooldown.on_ready();

        assert_eq!(cooldown.on_pending(), Outcome::Loop);
        assert_eq!(cooldown.on_pending(), Outcome::Loop);
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);
    }

    #[test]
    fn disabled_test() {
        let mut cooldown = Cooldown::new(0);

        // with cooldown disabled, it should always return sleep
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);

        cooldown.on_ready();
        assert_eq!(cooldown.on_pending(), Outcome::Sleep);
    }
}
