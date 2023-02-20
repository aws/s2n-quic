// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::units::{duration_format, Byte, ByteExt, Duration, DurationExt};
use core::fmt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Rate {
    pub bytes: Byte,
    #[serde(with = "duration_format", rename = "period_ms")]
    pub period: Duration,
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.period == 1.seconds() {
            return write!(f, "{}ps", self.bytes);
        }

        // force the period to be in seconds
        if f.alternate() {
            let factor = 1.0 / self.period.as_secs_f64();
            let bytes = (*self.bytes as f64 * factor) as u64;
            let bytes = bytes.bytes();
            return write!(f, "{bytes}ps");
        }

        write!(f, "{}/{:?}", self.bytes, self.period)
    }
}

impl core::str::FromStr for Rate {
    type Err = crate::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(bytes) = s.strip_suffix("ps") {
            return Ok(Self {
                bytes: bytes.parse()?,
                period: 1.seconds(),
            });
        }

        if let Some((bytes, period)) = s.split_once('/') {
            Ok(Self {
                bytes: bytes.trim().parse()?,
                period: *humantime::Duration::from_str(period.trim())?,
            })
        } else {
            Err(format!("invalid rate: {s}").into())
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Rates {
    pub send: HashMap<u64, Rate>,
    pub receive: HashMap<u64, Rate>,
}

#[cfg(test)]
mod tests {
    use crate::units::*;
    use insta::assert_debug_snapshot;

    #[test]
    fn ext_test() {
        assert_debug_snapshot!([
            42.bytes() / 42.millis(),
            42.mebibytes() / 42.seconds(),
            42.gigabytes() / 42.minutes()
        ]);
    }

    fn p(s: &str) -> crate::Result<Rate> {
        s.parse()
    }

    #[test]
    fn parse_test() {
        assert_debug_snapshot!([
            p("42bps"),
            p("42Bps"),
            p("42KBps"),
            p("42Kb / 50ms"),
            p("42Mb/5ms"),
        ]);
    }
}
