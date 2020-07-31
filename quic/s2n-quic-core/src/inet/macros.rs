macro_rules! define_inet_type {
    ($($vis:ident)? struct $name:ident {
        $(
            $field:ident: $field_ty:ty
        ),*
        $(,)*
    }) => {
        #[allow(unused_imports)]
        #[cfg(feature = "generator")]
        use bolero_generator::*;

        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, zerocopy::FromBytes, zerocopy::AsBytes, zerocopy::Unaligned)]
        #[cfg_attr(feature = "generator", derive(bolero_generator::TypeGenerator))]
        #[repr(C)]
        $($vis)? struct $name {
            $(
                pub(crate) $field: $field_ty,
            )*
        }

        impl $name {
            #[allow(non_camel_case_types, clippy::too_many_arguments)]
            pub fn new<$($field: Into<$field_ty>),*>($($field: $field),*) -> Self {
                Self {
                    $(
                        $field: $field.into()
                    ),*
                }
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
