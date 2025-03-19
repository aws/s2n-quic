// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! define_inet_type {
    ($($vis:ident)? struct $name:ident {
        $(
            $field:ident: $field_ty:ty
        ),*
        $(,)*
    }) => {
        #[allow(unused_imports)]
        #[cfg(any(test, feature = "generator"))]
        use bolero_generator::prelude::*;

        #[derive(Clone, Copy, Default, Eq, PartialEq, PartialOrd, Ord, zerocopy::FromBytes, zerocopy::IntoBytes, zerocopy::Unaligned, zerocopy::Immutable)]
        #[cfg_attr(any(test, feature = "generator"), derive(bolero_generator::TypeGenerator))]
        #[cfg_attr(kani, derive(kani::Arbitrary))]
        #[repr(C)]
        $($vis)? struct $name {
            $(
                pub(crate) $field: $field_ty,
            )*
        }

        // By letting the compiler derive PartialEq, we can do structural matching on these
        // structs. But we also want hashing to be on the byte level, since we know the struct
        // has no allocations and has a simple layout.
        //
        // See: https://godbolt.org/z/czohnrWxK
        #[allow(clippy::derived_hash_with_manual_eq)]
        impl core::hash::Hash for $name {
            #[inline]
            fn hash<H: core::hash::Hasher>(&self, hasher: &mut H) {
                self.as_bytes().hash(hasher);
            }
        }

        impl $name {
            #[allow(non_camel_case_types)]
            #[inline]
            pub fn new<$($field: Into<$field_ty>),*>($($field: $field),*) -> Self {
                Self {
                    $(
                        $field: $field.into()
                    ),*
                }
            }

            #[inline]
            pub fn as_bytes(&self) -> &[u8] {
                zerocopy::IntoBytes::as_bytes(self)
            }

            #[inline]
            pub fn as_bytes_mut(&mut self) -> &mut [u8] {
                zerocopy::IntoBytes::as_mut_bytes(self)
            }
        }

        s2n_codec::zerocopy_value_codec!($name);
    };
}

macro_rules! test_inet_snapshot {
    ($test:ident, $name:ident, $ty:ty) => {
        #[test]
        fn $name() {
            s2n_codec::assert_codec_round_trip_sample_file!(
                $ty,
                concat!("src/inet/test_samples/", stringify!($test), ".bin")
            );
        }
    };
}
