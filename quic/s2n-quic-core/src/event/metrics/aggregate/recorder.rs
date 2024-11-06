// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    info::{self, Info},
    Metric,
};
use core::time::Duration;

pub trait Recorder<T>: 'static + Send + Sync
where
    T: Metric,
{
    fn record(&self, info: &'static Info, value: T);
}

pub trait NominalRecorder<T>: 'static + Send + Sync
where
    T: Metric,
{
    fn record(&self, info: &'static Info, variant: &'static info::Variant, value: T);
}

macro_rules! impl_recorder {
    ($trait:ident $(, $extra_param:ident: $extra_type:ty)?) => {
        impl<A, B, T> $trait<T> for (A, B)
        where
            A: $trait<T>,
            B: $trait<T>,
            T: Metric,
        {
            #[inline]
            fn record(&self, info: &'static Info, $($extra_param: $extra_type,)? value: T) {
                self.0.record(info, $($extra_param,)? value);
                self.1.record(info, $($extra_param,)? value);
            }
        }

        impl<R, T> $trait<T> for Option<R>
        where
            R: $trait<T>,
            T: Metric,
        {
            #[inline]
            fn record(&self, info: &'static Info, $($extra_param: $extra_type,)? value: T) {
                if let Some(recorder) = self {
                    recorder.record(info, $($extra_param,)? value);
                }
            }
        }

        #[cfg(target_has_atomic = "64")]
        impl $trait<u64> for core::sync::atomic::AtomicU64 {
            #[inline]
            fn record(&self, _info: &'static Info, $($extra_param: $extra_type,)? value: u64) {
                self.fetch_add(value, core::sync::atomic::Ordering::Relaxed);
                $(let _ = $extra_param;)?
            }
        }

        #[cfg(target_has_atomic = "64")]
        impl $trait<Duration> for core::sync::atomic::AtomicU64 {
            #[inline]
            fn record(&self, _info: &'static Info, $($extra_param: $extra_type,)? value: Duration) {
                self.fetch_add(
                    value.as_micros() as _,
                    core::sync::atomic::Ordering::Relaxed,
                );
                $(let _ = $extra_param;)?
            }
        }

        #[cfg(feature = "alloc")]
        impl<R, T> $trait<T> for alloc::sync::Arc<R>
        where
            R: $trait<T>,
            T: Metric,
        {
            #[inline]
            fn record(&self, info: &'static Info, $($extra_param: $extra_type,)? value: T) {
                self.as_ref().record(info, $($extra_param,)? value );
            }
        }
    };
}

impl_recorder!(Recorder);
impl_recorder!(NominalRecorder, variant: &'static info::Variant);
