// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(test, feature = "testing"))]
use s2n_quic_core::event::snapshot;

pub use s2n_quic_core::event::{Event, IntoEvent, Timestamp};

mod generated;
pub use generated::*;
