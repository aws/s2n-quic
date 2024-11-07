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
    type BoolCounter = BoolCounter;
    type NominalCounter = NominalCounter;
    type Measure = Measure;
    type Gauge = Gauge;
    type Timer = Timer;

    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        Self::Counter::new(info)
    }

    #[inline]
    fn register_bool_counter(&self, info: &'static Info) -> Self::BoolCounter {
        Self::BoolCounter::new(info)
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
        $record:ident,
        $as_metric:ident : $metric_ty:ty
        $(, $variant:ident : $variant_ty:ty)?
    ) => {
        mod $module {
            use super::*;
            use crate::probe::define;
            use aggregate::Metric;

            define!(
                extern "probe" {
                    #[link_name = $register]
                    fn register(id: usize, name: &Str, units: &Str, $($variant: &Str)?);

                    #[link_name = $record]
                    fn record(id: usize, name: &Str, units: &Str, $($variant: &Str, )? value: $metric_ty);
                }
            );

            #[derive(Copy, Clone, Debug, Default)]
            pub struct $name(());

            impl $name {
                pub(super) fn new(info: &'static Info $(, $variant: $variant_ty)?) -> Self {
                    register(info.id, info.name, info.units.as_str(), $($variant.name)?);
                    Self(())
                }
            }

            impl aggregate::$recorder for $name {
                #[inline]
                fn record<T: Metric>(&self, info: &'static Info, $($variant: $variant_ty, )? value: T) {
                    record(info.id, info.name, info.units.as_str(), $($variant.name, )? value.$as_metric());
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
    s2n_quic__counter__record,
    as_u64: u64
);
recorder!(
    NominalRecorder,
    NominalCounter,
    nominal_counter,
    s2n_quic__counter__nominal__register,
    s2n_quic__counter__nominal__record,
    as_u64: u64,
    variant: &'static info::Variant
);
recorder!(
    Recorder,
    Measure,
    measure,
    s2n_quic__measure__register,
    s2n_quic__measure__record,
    as_u64: u64
);
recorder!(
    Recorder,
    Gauge,
    gauge,
    s2n_quic__gauge__register,
    s2n_quic__gauge__record,
    as_u64: u64
);
recorder!(
    Recorder,
    Timer,
    timer,
    s2n_quic__timer__register,
    s2n_quic__timer__record,
    as_duration: core::time::Duration
);

mod bool_counter {
    use super::*;
    use crate::probe::define;

    define!(
        extern "probe" {
            #[link_name = s2n_quic__counter__bool__register]
            fn register(id: usize, name: &Str);

            #[link_name = s2n_quic__counter__bool__record]
            fn record(id: usize, name: &Str, value: bool);
        }
    );

    #[derive(Copy, Clone, Debug, Default)]
    pub struct BoolCounter(());

    impl BoolCounter {
        pub(super) fn new(info: &'static Info) -> Self {
            register(info.id, info.name);
            Self(())
        }
    }

    impl aggregate::BoolRecorder for BoolCounter {
        #[inline]
        fn record(&self, info: &'static Info, value: bool) {
            record(info.id, info.name, value);
        }
    }
}

pub use bool_counter::BoolCounter;
