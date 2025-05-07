// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! impl_clock {
    () => {
        use super::SleepHandle;
        use core::{
            fmt,
            future::Future,
            pin::Pin,
            task::{Context, Poll},
            time::Duration,
        };
        use pin_project_lite::pin_project;
        use s2n_quic_core::{ready, time::Timestamp};
        use tracing::trace;

        #[derive(Clone, Debug)]
        pub struct Clock(Instant);

        impl Default for Clock {
            #[inline]
            fn default() -> Self {
                Self(Instant::now())
            }
        }

        impl s2n_quic_core::time::Clock for Clock {
            #[inline]
            fn get_time(&self) -> Timestamp {
                let time = self.0.elapsed();
                unsafe { Timestamp::from_duration(time) }
            }
        }

        pin_project!(
            pub struct Sleep {
                clock: Clock,
                #[pin]
                sleep: time::Sleep,
            }
        );

        impl s2n_quic_core::time::Clock for Sleep {
            #[inline]
            fn get_time(&self) -> Timestamp {
                self.clock.get_time()
            }
        }

        impl Future for Sleep {
            type Output = ();

            fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
                let this = self.project();
                ready!(core::future::Future::poll(this.sleep, cx));
                Poll::Ready(())
            }
        }

        impl super::Sleep for Sleep {
            #[inline]
            fn update(self: Pin<&mut Self>, target: Timestamp) {
                let target = unsafe { target.as_duration() };

                // floor the delay to milliseconds to reduce timer churn
                let delay = Duration::from_millis(target.as_millis() as u64);

                let target = self.clock.0 + delay;

                // if the clock has changed let the sleep future know
                trace!(update = ?target);
                self.project().sleep.reset(target);
            }
        }

        impl super::Clock for Sleep {
            #[inline]
            fn sleep(&self, amount: Duration) -> (SleepHandle, Timestamp) {
                self.clock.sleep(amount)
            }
        }

        impl fmt::Debug for Sleep {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct("Sleep")
                    .field("clock", &self.clock)
                    .field("sleep", &self.sleep)
                    .finish()
            }
        }

        impl super::Clock for Clock {
            #[inline]
            fn sleep(&self, amount: Duration) -> (SleepHandle, Timestamp) {
                let now = Instant::now();
                let sleep = sleep_until(now + amount);
                let sleep = Sleep {
                    clock: self.clone(),
                    sleep,
                };
                let sleep = Box::pin(sleep);
                let target = now.saturating_duration_since(self.0);
                let target = unsafe { Timestamp::from_duration(target) };
                (sleep, target)
            }
        }
    };
}
