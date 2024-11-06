// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::info::{Str, Variant};

pub trait AsVariant {
    const VARIANTS: &'static [Variant];

    fn variant_idx(&self) -> usize;

    #[inline]
    fn as_variant(&self) -> &'static Variant {
        &Self::VARIANTS[self.variant_idx()]
    }
}

impl AsVariant for bool {
    const VARIANTS: &'static [Variant] = &[
        Variant {
            name: Str::new("false\0"),
            id: 0,
        },
        Variant {
            name: Str::new("true\0"),
            id: 1,
        },
    ];

    #[inline]
    fn variant_idx(&self) -> usize {
        *self as usize
    }
}
