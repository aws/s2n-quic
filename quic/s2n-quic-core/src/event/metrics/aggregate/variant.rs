// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::info::Variant;

pub trait AsVariant {
    const VARIANTS: &'static [Variant];

    fn variant_idx(&self) -> usize;

    #[inline]
    fn as_variant(&self) -> &'static Variant {
        &Self::VARIANTS[self.variant_idx()]
    }
}
