// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::units::{Duration, Rate};
use core::ops;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize,
)]
pub struct Byte(u64);

impl Byte {
    pub const MAX: Self = Self(u64::MAX);
    pub const MIN: Self = Self(u64::MIN);
}

impl ops::Add<u64> for Byte {
    type Output = Self;

    fn add(self, rhs: u64) -> Self::Output {
        Self(self.0 + rhs)
    }
}

impl ops::AddAssign<u64> for Byte {
    fn add_assign(&mut self, rhs: u64) {
        self.0 += rhs;
    }
}

impl ops::Sub<u64> for Byte {
    type Output = Self;

    fn sub(self, rhs: u64) -> Self::Output {
        Self(self.0 - rhs)
    }
}

impl ops::SubAssign<u64> for Byte {
    fn sub_assign(&mut self, rhs: u64) {
        self.0 -= rhs;
    }
}

impl ops::Mul<u64> for Byte {
    type Output = Self;

    fn mul(self, rhs: u64) -> Self::Output {
        Self(self.0 * rhs)
    }
}

impl ops::Div<u64> for Byte {
    type Output = Self;

    fn div(self, rhs: u64) -> Self::Output {
        Self(self.0 / rhs)
    }
}

impl ops::Div<Duration> for Byte {
    type Output = Rate;

    fn div(self, period: Duration) -> Self::Output {
        Rate {
            bytes: self,
            period,
        }
    }
}

impl ops::Deref for Byte {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub trait ByteExt {
    fn bytes(&self) -> Byte;
    fn kilobytes(&self) -> Byte {
        self.bytes() * 1000
    }
    fn megabytes(&self) -> Byte {
        self.kilobytes() * 1000
    }
    fn gigabytes(&self) -> Byte {
        self.megabytes() * 1000
    }
}

impl ByteExt for i32 {
    fn bytes(&self) -> Byte {
        Byte(*self as _)
    }
}

impl ByteExt for u64 {
    fn bytes(&self) -> Byte {
        Byte(*self)
    }
}
