macro_rules! define_inet_type {
    ($($vis:ident)? struct $name:ident {
        $(
            $field:ident: $field_ty:ty
        ),*
        $(,)*
    }) => {
        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, zerocopy::FromBytes, zerocopy::AsBytes, zerocopy::Unaligned)]
        #[repr(C)]
        $($vis)? struct $name {
            $(
                $field: $field_ty,
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
            let file = concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/inet/test_samples/",
                stringify!($test),
                ".bin"
            );
            let mut expected =
                std::fs::read(file).unwrap_or_else(|_| panic!("could not open {:?}", file));
            let mut buffer = s2n_codec::DecoderBufferMut::new(&mut expected);
            let mut values = vec![];

            while !buffer.is_empty() {
                let (value, remaining) = buffer.decode::<$ty>().unwrap();
                s2n_codec::assert_codec_round_trip_value_mut!($ty, value);
                values.push(value);
                buffer = remaining;
            }

            insta::assert_debug_snapshot!(values);
        }
    };
}
