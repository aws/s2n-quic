// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// A trait to determine if the value is left unspecified,
/// usually containing the default value.
///
/// See: <https://en.wikipedia.org/wiki/IPv6_address#Unspecified_address>
pub trait Unspecified: Sized {
    /// Returns true if the value is unspecified
    fn is_unspecified(&self) -> bool;

    /// Coerce a potentially unspecified value into an `Option<Self>`
    fn filter_unspecified(self) -> Option<Self> {
        if self.is_unspecified() {
            None
        } else {
            Some(self)
        }
    }
}

macro_rules! unspecified_integer {
    ($name:ident) => {
        impl Unspecified for s2n_codec::zerocopy::$name {
            fn is_unspecified(&self) -> bool {
                Self::default().eq(self)
            }
        }
    };
}

unspecified_integer!(U16);
unspecified_integer!(U32);
unspecified_integer!(U64);
unspecified_integer!(U128);
