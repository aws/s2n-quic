// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "alloc")]
pub use crate::event::generated::metrics::aggregate::*;

pub mod info;
mod metric;
pub mod probe;
mod recorder;
mod variant;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use info::Info;
pub use metric::*;
pub use recorder::*;
pub use variant::*;

pub trait Registry: 'static + Send + Sync {
    type Counter: Recorder;
    type BoolCounter: BoolRecorder;
    type NominalCounter: NominalRecorder;
    type Measure: Recorder;
    type Gauge: Recorder;
    type Timer: Recorder;
    type NominalTimer: NominalRecorder;

    fn register_counter(&self, info: &'static Info) -> Self::Counter;

    fn register_bool_counter(&self, info: &'static Info) -> Self::BoolCounter;

    fn register_nominal_counter(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalCounter;

    fn register_measure(&self, info: &'static Info) -> Self::Measure;

    fn register_gauge(&self, info: &'static Info) -> Self::Gauge;

    fn register_timer(&self, info: &'static Info) -> Self::Timer;

    fn register_nominal_timer(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalTimer;
}

impl<A, B> Registry for (A, B)
where
    A: Registry,
    B: Registry,
{
    type Counter = (A::Counter, B::Counter);
    type BoolCounter = (A::BoolCounter, B::BoolCounter);
    type NominalCounter = (A::NominalCounter, B::NominalCounter);
    type Measure = (A::Measure, B::Measure);
    type Gauge = (A::Gauge, B::Gauge);
    type Timer = (A::Timer, B::Timer);
    type NominalTimer = (A::NominalTimer, B::NominalTimer);

    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        (self.0.register_counter(info), self.1.register_counter(info))
    }

    #[inline]
    fn register_bool_counter(&self, info: &'static Info) -> Self::BoolCounter {
        (
            self.0.register_bool_counter(info),
            self.1.register_bool_counter(info),
        )
    }

    #[inline]
    fn register_nominal_counter(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalCounter {
        (
            self.0.register_nominal_counter(info, variant),
            self.1.register_nominal_counter(info, variant),
        )
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

    #[inline]
    fn register_nominal_timer(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalTimer {
        (
            self.0.register_nominal_timer(info, variant),
            self.1.register_nominal_timer(info, variant),
        )
    }
}

#[cfg(feature = "alloc")]
impl<T: Registry> Registry for alloc::sync::Arc<T> {
    type Counter = T::Counter;
    type BoolCounter = T::BoolCounter;
    type NominalCounter = T::NominalCounter;
    type Measure = T::Measure;
    type Gauge = T::Gauge;
    type Timer = T::Timer;
    type NominalTimer = T::NominalTimer;

    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        self.as_ref().register_counter(info)
    }

    #[inline]
    fn register_bool_counter(&self, info: &'static Info) -> Self::BoolCounter {
        self.as_ref().register_bool_counter(info)
    }

    #[inline]
    fn register_nominal_counter(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalCounter {
        self.as_ref().register_nominal_counter(info, variant)
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

    #[inline]
    fn register_nominal_timer(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalTimer {
        self.as_ref().register_nominal_timer(info, variant)
    }
}
