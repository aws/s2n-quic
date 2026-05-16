// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{pin::Pin, time::Duration};

pub trait InstantHandle: Clone + fmt::Debug + Send + Sync + 'static {
    type Sleep;

    fn new() -> Self;
    fn elapsed_since_start(&self) -> Duration;
    fn sleep(&self, duration: Duration) -> (Self::Sleep, Duration);
    fn update_sleep(&self, sleep: Pin<&mut Self::Sleep>, since_start: Duration);
}

use core::fmt;

macro_rules! impl_clock {
    ($handle:ident) => {
        use super::{macros::InstantHandle as _, SleepHandle};
        use core::{
            fmt,
            future::Future,
            pin::Pin,
            task::{Context, Poll},
            time::Duration,
        };
        use pin_project_lite::pin_project;
        use s2n_quic_core::{ready, time::Timestamp};

        #[derive(Clone, Debug)]
        pub struct Clock($handle);

        impl Default for Clock {
            #[inline]
            fn default() -> Self {
                Self($handle::new())
            }
        }

        impl s2n_quic_core::time::Clock for Clock {
            #[inline]
            fn get_time(&self) -> Timestamp {
                let time = self.0.elapsed_since_start();
                unsafe { Timestamp::from_duration(time) }
            }
        }

        impl crate::time::precision::Clock for Clock {
            type Timer = super::Timer;

            #[inline]
            fn now(&self) -> crate::time::precision::Timestamp {
                let nanos = self.0.elapsed_since_start().as_nanos() as u64;
                crate::time::precision::Timestamp { nanos }
            }

            #[inline]
            fn timer(&self) -> Self::Timer {
                super::Timer::new(self)
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
                let since_start = unsafe { target.as_duration() };
                let this = self.project();
                this.clock.0.update_sleep(this.sleep, since_start);
            }
        }

        impl super::Clock for Sleep {
            #[inline]
            fn sleep(&self, amount: Duration) -> (SleepHandle, Timestamp) {
                self.clock.sleep(amount)
            }

            fn timer(&self) -> super::Timer {
                super::Timer::new(self)
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
                let (sleep, target) = self.0.sleep(amount);
                let sleep = Sleep {
                    clock: self.clone(),
                    sleep,
                };
                let sleep = Box::pin(sleep);
                let target = unsafe { Timestamp::from_duration(target) };
                (sleep, target)
            }

            fn timer(&self) -> super::Timer {
                super::Timer::new(self)
            }
        }
    };
}
