// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;

pub trait Metric: 'static + Send + Sync + Copy + core::fmt::Debug {}

impl Metric for u64 {}
impl Metric for f32 {}
impl Metric for f64 {}
impl Metric for Duration {}

pub trait AsMetric<T> {
    fn as_metric(&self, unit: &str) -> T;
}

macro_rules! impl_as_metric_number {
    ($($ty:ty),* $(,)?) => {
        $(
            impl AsMetric<u64> for $ty {
                #[inline]
                fn as_metric(&self, _unit: &str) -> u64 {
                    *self as _
                }
            }

            impl AsMetric<f32> for $ty {
                #[inline]
                fn as_metric(&self, _unit: &str) -> f32 {
                    *self as _
                }
            }

            impl AsMetric<f64> for $ty {
                #[inline]
                fn as_metric(&self, _unit: &str) -> f64 {
                    *self as _
                }
            }
        )*
    }
}

impl_as_metric_number!(u8, u16, u32, u64, usize);

impl AsMetric<u64> for Duration {
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

impl AsMetric<Duration> for Duration {
    #[inline]
    fn as_metric(&self, _unit: &str) -> Duration {
        *self
    }
}
