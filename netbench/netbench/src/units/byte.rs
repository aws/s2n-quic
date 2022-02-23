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

impl ops::Add for Byte {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl ops::AddAssign for Byte {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl ops::Sub for Byte {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}

impl ops::SubAssign for Byte {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
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
    fn kibibytes(&self) -> Byte {
        self.bytes() * 1024
    }
    fn megabytes(&self) -> Byte {
        self.kilobytes() * 1000
    }
    fn mebibytes(&self) -> Byte {
        self.kibibytes() * 1024
    }
    fn gigabytes(&self) -> Byte {
        self.megabytes() * 1000
    }
    fn gibibytes(&self) -> Byte {
        self.mebibytes() * 1024
    }
}

impl ByteExt for u64 {
    fn bytes(&self) -> Byte {
        Byte(*self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn ext_test() {
        assert_debug_snapshot!([
            42.bytes(),
            42.kilobytes(),
            42.kibibytes(),
            42.megabytes(),
            42.mebibytes(),
            42.gigabytes(),
            42.gibibytes(),
            42.kibibytes() + 42.bytes()
        ]);
    }
}
