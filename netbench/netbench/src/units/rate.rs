// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::units::{duration_format, Byte, Duration};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Hash, Deserialize, Serialize)]
pub struct Rate {
    pub bytes: Byte,
    #[serde(with = "duration_format", rename = "period_ms")]
    pub period: Duration,
}

#[derive(Debug, Default)]
pub(crate) struct Rates {
    pub send: HashMap<u64, Rate>,
    pub receive: HashMap<u64, Rate>,
}
