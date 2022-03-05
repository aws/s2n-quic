// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::{Context, Poll};
pub use s2n_quic_core::time::{
    timer::{Provider, Query, Result},
    Timestamp,
};

pub trait Timer {
    fn now(&self) -> Timestamp;
    fn poll(&mut self, target: Timestamp, cx: &mut Context) -> Poll<()>;
}

#[cfg(any(test, feature = "testing"))]
mod testing;
mod tokio;

pub use self::tokio::Timer as Tokio;
#[cfg(any(test, feature = "testing"))]
pub use testing::Timer as Testing;
