// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use core::time::Duration;

pub fn parse_duration(s: &str) -> crate::Result<Duration> {
    Ok(humantime::parse_duration(s)?)
}

pub trait DurationExt {
    fn millis(&self) -> Duration;
    fn seconds(&self) -> Duration {
        self.millis() * 1000
    }
    fn minutes(&self) -> Duration {
        self.seconds() * 60
    }
}

impl DurationExt for f32 {
    fn millis(&self) -> Duration {
        Duration::from_secs_f32(*self / 1000.0)
    }
    fn seconds(&self) -> Duration {
        Duration::from_secs_f32(*self)
    }
}

impl DurationExt for u64 {
    fn millis(&self) -> Duration {
        Duration::from_millis(*self)
    }
}

pub(crate) mod duration_format {
    use core::time::Duration;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u64(duration.as_millis() as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let millis = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(millis))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn ext_test() {
        assert_debug_snapshot!([
            42.millis(),
            42.seconds(),
            42.minutes(),
            42.minutes() + 42.seconds(),
            4.2.millis(),
            4.2.seconds(),
            4.2.minutes(),
            4.2.minutes() + 4.2.seconds(),
        ]);
    }
}
