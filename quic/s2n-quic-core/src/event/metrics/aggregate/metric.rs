// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::info::Str;
use core::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Units {
    None,
    Bytes,
    Duration,
}

impl Units {
    pub const fn as_str(&self) -> &'static Str {
        match self {
            Units::None => Str::new("\0"),
            Units::Bytes => Str::new("bytes\0"),
            Units::Duration => Str::new("duration\0"),
        }
    }
}

pub trait Metric: 'static + Send + Sync + Copy + core::fmt::Debug {
    #[inline]
    fn is_f32(&self) -> bool {
        false
    }
    fn as_f32(&self) -> f32;

    #[inline]
    fn is_f64(&self) -> bool {
        false
    }
    fn as_f64(&self) -> f64;

    #[inline]
    fn is_u64(&self) -> bool {
        false
    }
    fn as_u64(&self) -> u64;

    #[inline]
    fn is_duration(&self) -> bool {
        false
    }
    fn as_duration(&self) -> Duration;
}

impl Metric for f32 {
    #[inline]
    fn as_f32(&self) -> f32 {
        *self
    }

    #[inline]
    fn is_f32(&self) -> bool {
        true
    }

    #[inline]
    fn as_f64(&self) -> f64 {
        *self as _
    }

    #[inline]
    fn as_u64(&self) -> u64 {
        *self as _
    }

    #[inline]
    fn as_duration(&self) -> Duration {
        Duration::from_secs_f32(*self)
    }
}

impl Metric for f64 {
    #[inline]
    fn as_f32(&self) -> f32 {
        *self as _
    }

    #[inline]
    fn as_f64(&self) -> f64 {
        *self
    }

    #[inline]
    fn is_f64(&self) -> bool {
        true
    }

    #[inline]
    fn as_u64(&self) -> u64 {
        *self as _
    }

    #[inline]
    fn as_duration(&self) -> Duration {
        Duration::from_secs_f64(*self)
    }
}

impl Metric for Duration {
    #[inline]
    fn as_f32(&self) -> f32 {
        self.as_secs_f32()
    }

    #[inline]
    fn as_f64(&self) -> f64 {
        self.as_secs_f64()
    }

    #[inline]
    fn as_u64(&self) -> u64 {
        self.as_micros() as _
    }

    #[inline]
    fn is_duration(&self) -> bool {
        true
    }

    #[inline]
    fn as_duration(&self) -> Duration {
        *self
    }
}

macro_rules! impl_metric_number {
    ($($ty:ty),* $(,)?) => {
        $(
            impl Metric for $ty {
                #[inline]
                fn as_f32(&self) -> f32 {
                    *self as _
                }

                #[inline]
                fn as_f64(&self) -> f64 {
                    *self as _
                }

                #[inline]
                fn is_u64(&self) -> bool {
                    true
                }

                #[inline]
                fn as_u64(&self) -> u64 {
                    *self as _
                }

                #[inline]
                fn as_duration(&self) -> Duration {
                    Duration::from_micros(*self as _)
                }
            }
        )*
    }
}

impl_metric_number!(u8, u16, u32, u64, usize);
