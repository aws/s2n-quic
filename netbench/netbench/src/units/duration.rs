// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use core::time::Duration;

pub trait DurationExt {
    fn millis(&self) -> Duration;
    fn seconds(&self) -> Duration {
        self.millis() * 1000
    }
    fn minutes(&self) -> Duration {
        self.seconds() * 60
    }
}

impl DurationExt for i32 {
    fn millis(&self) -> Duration {
        Duration::from_millis(*self as _)
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
