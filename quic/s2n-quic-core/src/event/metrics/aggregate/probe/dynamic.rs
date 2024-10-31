// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::event::metrics::aggregate::{self, info::Str, Info};

#[derive(Clone, Debug, Default)]
pub struct Registry(());

impl aggregate::Registry for Registry {
    type Counter = Counter;
    type Measure = Measure;
    type Gauge = Gauge;
    type Timer = Measure;

    #[inline]
    fn register_counter(&self, info: &'static Info) -> Self::Counter {
        Self::Counter::new(info)
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
    ($name:ident, $module:ident, $register:ident, $record:ident) => {
        mod $module {
            use super::*;
            use crate::probe::define;

            define!(
                extern "probe" {
                    #[link_name = $register]
                    fn register(id: usize, name: Str, units: Str);

                    #[link_name = $record]
                    fn record(id: usize, name: Str, units: Str, value: u64);
                }
            );

            #[derive(Copy, Clone, Debug, Default)]
            pub struct $name(());

            impl $name {
                pub(super) fn new(info: &'static Info) -> Self {
                    register(info.id, info.name, info.units);
                    Self(())
                }
            }

            impl aggregate::Recorder for $name {
                #[inline]
                fn record(&self, info: &'static Info, value: u64) {
                    record(info.id, info.name, info.units, value);
                }
            }
        }

        pub use $module::$name;
    };
}

recorder!(
    Counter,
    counter,
    s2n_quic__counter__register,
    s2n_quic__counter__record
);
recorder!(
    Measure,
    measure,
    s2n_quic__measure__register,
    s2n_quic__measure__record
);
recorder!(
    Gauge,
    gauge,
    s2n_quic__gauge__register,
    s2n_quic__gauge__record
);
