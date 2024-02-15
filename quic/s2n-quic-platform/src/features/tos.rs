// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::c_int;
use s2n_quic_core::inet::ExplicitCongestionNotification;

pub const IS_SUPPORTED: bool = super::tos_v4::IS_SUPPORTED || super::tos_v6::IS_SUPPORTED;

#[inline]
pub const fn is_match(level: c_int, ty: c_int) -> bool {
    super::tos_v4::is_match(level, ty) || super::tos_v6::is_match(level, ty)
}

#[inline]
pub fn decode(bytes: &[u8]) -> Option<ExplicitCongestionNotification> {
    let value = match bytes.len() {
        1 => bytes[0],
        4 => u32::from_ne_bytes(bytes.try_into().unwrap()) as u8,
        _ => return None,
    };

    Some(ExplicitCongestionNotification::new(value))
}
