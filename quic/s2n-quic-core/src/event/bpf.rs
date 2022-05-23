// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::api;
use core::time::Duration;

pub use super::generated::bpf::Subscriber;

pub(super) trait IntoBpf<Target> {
    fn as_bpf(&self) -> Target;
}

// TODO: for simplification all fields are u64... figure out a
// mechanism to expose other types. Possibly ready the IntoBpf
// definitions.
macro_rules! ident_into_bpf {
    ($($name:ty),* $(,)?) => {
        $(
            impl IntoBpf<u64> for $name {
                #[inline]
                fn as_bpf(&self) -> u64 {
                    *self as u64
                }
            }
        )*
    };
}

ident_into_bpf!(u8, u16, u32, u64, usize);

impl IntoBpf<u64> for Duration {
    fn as_bpf(&self) -> u64 {
        self.as_nanos() as u64
    }
}

impl IntoBpf<u64> for api::Path<'_> {
    fn as_bpf(&self) -> u64 {
        self.id
    }
}
