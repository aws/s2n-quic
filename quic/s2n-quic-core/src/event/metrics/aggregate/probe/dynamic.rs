// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::metrics::aggregate::{
    self,
    info::{self, Str},
    Info,
};

#[derive(Clone, Debug, Default)]
pub struct Registry(());

impl aggregate::Registry for Registry {
    type Counter = Counter;
    type NominalCounter = NominalCounter;
    type Measure = Measure;
    type Gauge = Gauge;
    type Timer = Measure;

    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        Self::Counter::new(info)
    }

    #[inline]
    fn register_nominal_counter(
        &self,
        info: &'static Info,
        variant: &'static info::Variant,
    ) -> Self::NominalCounter {
        Self::NominalCounter::new(info, variant)
    }

    #[inline]
    fn register_measure(&self, info: &'static Info) -> Self::Measure {
        Self::Measure::new(info)
    }

    #[inline]
    fn register_gauge(&self, info: &'static Info) -> Self::Gauge {
        Self::Gauge::new(info)
    }

    #[inline]
    fn register_timer(&self, info: &'static Info) -> Self::Timer {
        Self::Timer::new(info)
    }
}

macro_rules! recorder {
    (
        $recorder:ident,
        $name:ident,
        $module:ident,
        $register:ident,
        $record:ident
        $(, $variant:ident : $variant_ty:ty)?
    ) => {
        mod $module {
            use super::*;
            use crate::probe::define;
            use aggregate::AsMetric;
            use core::time::Duration;

            define!(
                extern "probe" {
                    #[link_name = $register]
                    fn register(id: usize, name: &Str, units: &Str, $($variant: &Str)?);

                    #[link_name = $record]
                    fn record(id: usize, name: &Str, units: &Str, $($variant: &Str, )? value: u64);
                }
            );

            #[derive(Copy, Clone, Debug, Default)]
            pub struct $name(());

            impl $name {
                pub(super) fn new(info: &'static Info $(, $variant: $variant_ty)?) -> Self {
                    register(info.id, info.name, info.units $(, $variant.name)?);
                    Self(())
                }
            }

            impl aggregate::$recorder<u64> for $name {
                #[inline]
                fn record(&self, info: &'static Info, $($variant: $variant_ty, )? value: u64) {
                    record(info.id, info.name, info.units, $($variant.name, )? value);
                }
            }

            impl aggregate::$recorder<Duration> for $name {
                #[inline]
                fn record(&self, info: &'static Info, $($variant: $variant_ty, )? value: Duration) {
                    record(info.id, info.name, info.units, $($variant.name, )? value.as_metric(info.units));
                }
            }
        }

        pub use $module::$name;
    };
}

recorder!(
    Recorder,
    Counter,
    counter,
    s2n_quic__counter__register,
    s2n_quic__counter__record
);
recorder!(
    NominalRecorder,
    NominalCounter,
    nominal_counter,
    s2n_quic__nominal_counter__register,
    s2n_quic__nominal_counter__record,
    variant: &'static info::Variant
);
recorder!(
    Recorder,
    Measure,
    measure,
    s2n_quic__measure__register,
    s2n_quic__measure__record
);
recorder!(
    Recorder,
    Gauge,
    gauge,
    s2n_quic__gauge__register,
    s2n_quic__gauge__record
);
