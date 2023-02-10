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
        use bolero_generator::*;

        #[derive(Clone, Copy, Default, Eq, zerocopy::FromBytes, zerocopy::AsBytes, zerocopy::Unaligned)]
        #[cfg_attr(any(test, feature = "generator"), derive(bolero_generator::TypeGenerator))]
        #[repr(C)]
        $($vis)? struct $name {
            $(
                pub(crate) $field: $field_ty,
            )*
        }

        impl PartialEq for $name {
            #[inline]
            fn eq(&self, other: &Self) -> bool {
                self.as_bytes().eq(other.as_bytes())
            }
        }

        impl PartialOrd for $name {
            #[inline]
            fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            #[inline]
            fn cmp(&self, other: &Self) -> core::cmp::Ordering {
                self.as_bytes().cmp(other.as_bytes())
            }
        }

        impl core::hash::Hash for $name {
            #[inline]
            fn hash<H: core::hash::Hasher>(&self, hasher: &mut H) {
                self.as_bytes().hash(hasher);
            }
        }

        impl $name {
            #[allow(non_camel_case_types, clippy::too_many_arguments)]
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
                zerocopy::AsBytes::as_bytes(self)
            }

            #[inline]
            pub fn as_bytes_mut(&mut self) -> &mut [u8] {
                zerocopy::AsBytes::as_bytes_mut(self)
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
