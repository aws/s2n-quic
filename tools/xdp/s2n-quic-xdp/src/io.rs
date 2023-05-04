// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// TODO replace with `core::task::ready` once we bump MSRV to 1.64.0
// https://doc.rust-lang.org/core/task/macro.ready.html
//
// See https://github.com/aws/s2n-quic/issues/1750
macro_rules! ready {
    ($value:expr) => {
        match $value {
            ::core::task::Poll::Ready(v) => v,
            ::core::task::Poll::Pending => {
                return ::core::task::Poll::Pending;
            }
        }
    };
}

pub mod rx;
pub mod tx;

#[cfg(test)]
mod tests;
