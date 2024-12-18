// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{
    info::{self, Info},
    Metric,
};

pub trait Recorder: 'static + Send + Sync {
    fn record<T: Metric>(&self, info: &'static Info, value: T);
}

pub trait BoolRecorder: 'static + Send + Sync {
    fn record(&self, info: &'static Info, value: bool);
}

pub trait NominalRecorder: 'static + Send + Sync {
    fn record<T: Metric>(&self, info: &'static Info, variant: &'static info::Variant, value: T);
}

macro_rules! impl_recorder {
    ($trait:ident $(, $extra_param:ident: $extra_type:ty)?) => {
        impl<A, B> $trait for (A, B)
        where
            A: $trait,
            B: $trait,
        {
            #[inline]
            fn record<T: Metric>(&self, info: &'static Info, $($extra_param: $extra_type,)? value: T) {
                self.0.record(info, $($extra_param,)? value);
                self.1.record(info, $($extra_param,)? value);
            }
        }

        impl<R> $trait for Option<R>
        where
            R: $trait,
        {
            #[inline]
            fn record<T: Metric>(&self, info: &'static Info, $($extra_param: $extra_type,)? value: T) {
                if let Some(recorder) = self {
                    recorder.record(info, $($extra_param,)? value);
                }
            }
        }

        #[cfg(target_has_atomic = "64")]
        impl $trait for core::sync::atomic::AtomicU64 {
            #[inline]
            fn record<T: Metric>(&self, _info: &'static Info, $($extra_param: $extra_type,)? value: T) {
                self.fetch_add(value.as_u64(), core::sync::atomic::Ordering::Relaxed);
                $(let _ = $extra_param;)?
            }
        }

        #[cfg(feature = "alloc")]
        impl<R> $trait for alloc::sync::Arc<R>
        where
            R: $trait,
        {
            #[inline]
            fn record<T: Metric>(&self, info: &'static Info, $($extra_param: $extra_type,)? value: T) {
                self.as_ref().record(info, $($extra_param,)? value );
            }
        }
    };
}

impl_recorder!(Recorder);
impl_recorder!(NominalRecorder, variant: &'static info::Variant);

impl<A, B> BoolRecorder for (A, B)
where
    A: BoolRecorder,
    B: BoolRecorder,
{
    #[inline]
    fn record(&self, info: &'static Info, value: bool) {
        self.0.record(info, value);
        self.1.record(info, value);
    }
}

impl<A> BoolRecorder for Option<A>
where
    A: BoolRecorder,
{
    #[inline]
    fn record(&self, info: &'static Info, value: bool) {
        if let Some(recorder) = self {
            recorder.record(info, value);
        }
    }
}

#[cfg(target_has_atomic = "64")]
impl BoolRecorder for core::sync::atomic::AtomicU64 {
    #[inline]
    fn record(&self, _info: &'static Info, value: bool) {
        if value {
            self.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
    }
}

#[cfg(feature = "alloc")]
impl<R> BoolRecorder for alloc::sync::Arc<R>
where
    R: BoolRecorder,
{
    #[inline]
    fn record(&self, info: &'static Info, value: bool) {
        self.as_ref().record(info, value);
    }
}
