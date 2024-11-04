// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "alloc")]
pub use crate::event::generated::metrics::aggregate::*;

pub mod info;
pub mod probe;
pub use info::Info;

pub trait Registry: 'static + Send + Sync {
    type Counter: Recorder;
    type Measure: Recorder;
    type Gauge: Recorder;
    type Timer: Recorder;

    fn register_counter(&self, info: &'static Info) -> Self::Counter;

    fn register_measure(&self, info: &'static Info) -> Self::Measure;

    fn register_gauge(&self, info: &'static Info) -> Self::Gauge;

    fn register_timer(&self, info: &'static Info) -> Self::Timer;
}

impl<A, B> Registry for (A, B)
where
    A: Registry,
    B: Registry,
{
    type Counter = (A::Counter, B::Counter);
    type Measure = (A::Measure, B::Measure);
    type Gauge = (A::Gauge, B::Gauge);
    type Timer = (A::Timer, B::Timer);

    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        (self.0.register_counter(info), self.1.register_counter(info))
    }

    #[inline]
    fn register_measure(&self, info: &'static Info) -> Self::Measure {
        (self.0.register_measure(info), self.1.register_measure(info))
    }

    #[inline]
    fn register_gauge(&self, info: &'static Info) -> Self::Gauge {
        (self.0.register_gauge(info), self.1.register_gauge(info))
    }

    #[inline]
    fn register_timer(&self, info: &'static Info) -> Self::Timer {
        (self.0.register_timer(info), self.1.register_timer(info))
    }
}

#[cfg(feature = "alloc")]
impl<T: Registry> Registry for alloc::sync::Arc<T> {
    type Counter = T::Counter;
    type Measure = T::Measure;
    type Gauge = T::Gauge;
    type Timer = T::Timer;

    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        self.as_ref().register_counter(info)
    }

    #[inline]
    fn register_measure(&self, info: &'static Info) -> Self::Measure {
        self.as_ref().register_measure(info)
    }

    #[inline]
    fn register_gauge(&self, info: &'static Info) -> Self::Gauge {
        self.as_ref().register_gauge(info)
    }

    #[inline]
    fn register_timer(&self, info: &'static Info) -> Self::Timer {
        self.as_ref().register_timer(info)
    }
}

pub trait Recorder: 'static + Send + Sync {
    fn record(&self, info: &'static Info, value: u64);
}

impl<A, B> Recorder for (A, B)
where
    A: Recorder,
    B: Recorder,
{
    #[inline]
    fn record(&self, info: &'static Info, value: u64) {
        self.0.record(info, value);
        self.1.record(info, value);
    }
}

#[cfg(target_has_atomic = "64")]
impl Recorder for core::sync::atomic::AtomicU64 {
    #[inline]
    fn record(&self, _info: &'static Info, value: u64) {
        self.fetch_add(value, core::sync::atomic::Ordering::Relaxed);
    }
}

#[cfg(feature = "alloc")]
impl<T: Recorder> Recorder for alloc::sync::Arc<T> {
    #[inline]
    fn record(&self, info: &'static Info, value: u64) {
        self.as_ref().record(info, value);
    }
}

pub trait AsMetric {
    fn as_metric(&self, unit: &str) -> u64;
}

macro_rules! impl_as_metric_number {
    ($($ty:ty),* $(,)?) => {
        $(
            impl AsMetric for $ty {
                #[inline]
                fn as_metric(&self, _unit: &str) -> u64 {
                    *self as _
                }
            }
        )*
    }
}

impl_as_metric_number!(u8, u16, u32, u64, usize);

impl AsMetric for core::time::Duration {
    #[inline]
    fn as_metric(&self, unit: &str) -> u64 {
        match unit {
            "s" => self.as_secs(),
            "ms" => self.as_millis().try_into().unwrap_or(u64::MAX),
            "us" => self.as_micros().try_into().unwrap_or(u64::MAX),
            "ns" => self.as_nanos().try_into().unwrap_or(u64::MAX),
            _ => panic!("invalid unit - {unit:?}"),
        }
    }
}
