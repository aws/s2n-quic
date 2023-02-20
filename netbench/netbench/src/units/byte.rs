// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::units::{Duration, Rate};
use core::{fmt, ops};
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

impl fmt::Display for Byte {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let opts = humansize::file_size_opts::FileSizeOpts {
            space: false,
            ..humansize::file_size_opts::DECIMAL
        };
        humansize::FileSize::file_size(&self.0, opts)
            .unwrap()
            .fmt(f)
    }
}

impl core::str::FromStr for Byte {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // try parsing without the suffix
        if let Ok(v) = s.parse() {
            return Ok(Self(v));
        }

        let number_index = s
            .char_indices()
            .find_map(|(idx, c)| {
                if !(c.is_numeric() || c == '.') {
                    Some(idx)
                } else {
                    None
                }
            })
            .unwrap_or(s.len());

        let mut v = Self(s[..number_index].parse()?);

        let mut suffix = s[number_index..].trim();
        let mut is_bits = false;

        if let Some(s) = suffix.strip_suffix('B') {
            suffix = s;
        } else if let Some(s) = suffix.strip_suffix('b') {
            is_bits = true;
            suffix = s;
        }

        v.0 *= *match suffix.trim() {
            "" => 1.bytes(),
            "K" | "k" => 1.kilobytes(),
            "Ki" | "ki" => 1.kibibytes(),
            "M" | "m" => 1.megabytes(),
            "Mi" | "mi" => 1.mebibytes(),
            "G" | "g" => 1.gigabytes(),
            "Gi" | "gi" => 1.gibibytes(),
            "T" | "t" => 1.terabytes(),
            "Ti" | "ti" => 1.tebibytes(),
            _ => return Err(format!("invalid bytes: {s:?}").into()),
        };

        if is_bits {
            // round up to the nearest byte
            if v.0 % 8 != 0 {
                v += 8.bytes();
            }
            v.0 /= 8;
        }

        Ok(v)
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
    fn terabytes(&self) -> Byte {
        self.gigabytes() * 1000
    }
    fn tebibytes(&self) -> Byte {
        self.gibibytes() * 1024
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

    fn p(s: &str) -> crate::Result<Byte> {
        s.parse()
    }

    #[test]
    fn parse_test() {
        assert_debug_snapshot!([
            p("42b"),
            p("42B"),
            p("42Kb"),
            p("42KB"),
            p("42Kib"),
            p("42KiB"),
            p("42Mb"),
            p("42MB"),
            p("42Mib"),
            p("42MiB"),
            p("42Gb"),
            p("42GB"),
            p("42Gib"),
            p("42GiB"),
            p("42Tb"),
            p("42TB"),
            p("42Tib"),
            p("42TiB"),
        ]);
    }
}
