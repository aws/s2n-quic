// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{fmt, marker::PhantomData, str::FromStr, time::Duration};
use s2n_quic::provider::io::testing::rand;
use serde::Deserialize;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CliRange<T> {
    pub start: T,
    pub end: T,
}

impl Default for CliRange<f64> {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: 0.0,
        }
    }
}

impl Default for CliRange<u64> {
    fn default() -> Self {
        Self { start: 0, end: 0 }
    }
}

impl Default for CliRange<humantime::Duration> {
    fn default() -> Self {
        Self {
            start: Duration::ZERO.into(),
            end: Duration::ZERO.into(),
        }
    }
}

impl<T: PartialEq + fmt::Display> fmt::Display for CliRange<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.start == self.end {
            self.start.fmt(f)
        } else {
            write!(f, "{}..{}", self.start, self.end)
        }
    }
}

impl<T> CliRange<T>
where
    T: Copy + PartialOrd + ::rand::distributions::uniform::SampleUniform,
{
    pub fn gen(&self) -> T {
        if self.start == self.end {
            return self.start;
        }

        rand::gen_range(self.start..self.end)
    }
}

impl CliRange<humantime::Duration> {
    pub fn gen_duration(&self) -> Duration {
        let start = self.start.as_nanos();
        let end = self.end.as_nanos();

        if start == end {
            return Duration::from_nanos(start as _);
        }

        let nanos = rand::gen_range(start..end);
        Duration::from_nanos(nanos as _)
    }
}

impl<T: Copy + FromStr> FromStr for CliRange<T> {
    type Err = T::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((start, end)) = s.split_once("..") {
            let start = start.parse()?;
            let end = end.parse()?;
            Ok(Self { start, end })
        } else {
            let start = s.parse()?;
            let end = start;
            Ok(Self { start, end })
        }
    }
}

impl<'de, T> Deserialize<'de> for CliRange<T>
where
    T: Copy + FromStr,
    <T as FromStr>::Err: core::fmt::Display,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(Visitor::<T>(PhantomData))
    }
}

struct Visitor<T>(PhantomData<T>);

impl<'de, T> serde::de::Visitor<'de> for Visitor<T>
where
    T: Copy + FromStr,
    <T as FromStr>::Err: core::fmt::Display,
{
    type Value = CliRange<T>;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a range or individual value")
    }

    fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        v.to_string().parse().map_err(E::custom)
    }

    fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        v.to_string().parse().map_err(E::custom)
    }

    fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        v.to_string().parse().map_err(E::custom)
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        v.parse().map_err(E::custom)
    }
}
