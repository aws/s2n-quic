// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use crate::{
    DecoderBuffer, DecoderBufferMut, DecoderError, DecoderValue, DecoderValueMut, Encoder,
    EncoderBuffer, EncoderLenEstimator, EncoderValue,
};

pub type Error = DecoderError;

#[macro_export]
macro_rules! assert_codec_round_trip_value {
    ($ty:ty, $value:expr) => {{
        $crate::ensure_codec_round_trip_value!($ty, $value).unwrap()
    }};
}

#[macro_export]
macro_rules! ensure_codec_round_trip_value {
    ($ty:ty, $value:expr) => {{
        fn execute_round_trip(expected_value: &$ty) -> Result<Vec<u8>, $crate::testing::Error> {
            let mut expected_bytes = $crate::testing::encode(expected_value)?;
            $crate::testing::ensure_decoding_matches(expected_value, &expected_bytes)?;
            $crate::testing::ensure_decoding_mut_matches(expected_value, &mut expected_bytes)?;
            Ok(expected_bytes)
        }

        execute_round_trip(&$value)
    }};
}

#[macro_export]
macro_rules! assert_codec_round_trip_value_mut {
    ($ty:ty, $value:expr) => {{
        $crate::ensure_codec_round_trip_value_mut!($ty, $value).unwrap()
    }};
}

#[macro_export]
macro_rules! ensure_codec_round_trip_value_mut {
    ($ty:ty, $value:expr) => {{
        fn execute_round_trip(expected_value: &$ty) -> Result<Vec<u8>, $crate::testing::Error> {
            let mut expected_bytes = $crate::testing::encode(expected_value)?;
            $crate::testing::ensure_decoding_mut_matches(expected_value, &mut expected_bytes)?;
            Ok(expected_bytes)
        }

        execute_round_trip(&$value)
    }};
}

macro_rules! impl_round_trip_bytes {
    ($name:ident, $round_trip:ident, $buffer:ident, $slice:ty, $as_ref:ident) => {
        #[macro_export]
        macro_rules! $name {
            ($ty: ty,$bytes: expr) => {{
                fn execute_all_round_trip(
                    buffer: $slice,
                ) -> Result<Vec<$ty>, $crate::testing::Error> {
                    let mut buffer = $crate::$buffer::new(buffer);
                    let mut values = vec![];

                    while let Ok((value, remaining)) = buffer.decode::<$ty>() {
                        $crate::$round_trip!($ty, &value)?;
                        values.push(value);
                        buffer = remaining;

                        // handle zero-sized encoded types, otherwise we loop forever
                        if buffer.is_empty() {
                            break;
                        }
                    }

                    Ok(values)
                }

                execute_all_round_trip($bytes.$as_ref())
            }};
        }
    };
}

#[macro_export]
macro_rules! assert_codec_round_trip_bytes {
    ($ty:ty, $bytes:expr) => {{
        $crate::ensure_codec_round_trip_bytes!($ty, $bytes).unwrap()
    }};
}

impl_round_trip_bytes!(
    ensure_codec_round_trip_bytes,
    ensure_codec_round_trip_value,
    DecoderBuffer,
    &[u8],
    as_ref
);

#[macro_export]
macro_rules! assert_codec_round_trip_bytes_mut {
    ($ty:ty, $bytes:expr) => {{
        $crate::ensure_codec_round_trip_bytes_mut!($ty, $bytes).unwrap()
    }};
}

impl_round_trip_bytes!(
    ensure_codec_round_trip_bytes_mut,
    ensure_codec_round_trip_value_mut,
    DecoderBufferMut,
    &mut [u8],
    as_mut
);

#[macro_export]
macro_rules! assert_codec_round_trip_sample_file {
    ($ty:ty, $path:expr) => {{
        $crate::assert_codec_round_trip_sample_file!($ty, $path, |buffer| {
            let (value, buffer) = buffer.decode::<$ty>().unwrap();
            $crate::assert_codec_round_trip_value_mut!($ty, value);
            (value, buffer)
        });
    }};
    ($ty:ty, $path:expr, | $buffer:ident | $decode:expr) => {{
        #[cfg(not(miri))]
        let mut expected = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/", $path)).unwrap();
        #[cfg(miri)]
        let mut expected = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/", $path)).to_vec();

        let mut $buffer = $crate::DecoderBufferMut::new(&mut expected);
        let mut values = vec![];

        while !$buffer.is_empty() {
            let (value, remaining) = $decode;
            values.push(value);
            $buffer = remaining;
        }

        #[cfg(not(miri))]
        insta::assert_debug_snapshot!(
            std::path::Path::new($path)
                .file_stem()
                .unwrap()
                .to_str()
                .unwrap(),
            values
        );
    }};
}

macro_rules! ensure {
    ($expr:expr, $message:expr $(,)?) => {
        if !($expr) {
            return Err($crate::testing::Error::InvariantViolation($message));
        }
    };
}

pub fn encode<T: EncoderValue>(expected_value: &T) -> Result<Vec<u8>, Error> {
    let len = expected_value.encoding_size();
    let mut buffer = vec![0; len];
    EncoderBuffer::new(&mut buffer).encode(expected_value);
    Ok(buffer)
}

pub fn ensure_encoding_matches<T: EncoderValue + PartialEq + core::fmt::Debug>(
    expected_value: &T,
    expected_bytes: &[u8],
) -> Result<Vec<u8>, Error> {
    let actual_bytes = encode(expected_value)?;
    ensure!(actual_bytes == expected_bytes, "encodings do not match");
    Ok(actual_bytes)
}

pub fn decode<'a, T: DecoderValue<'a>>(
    expected_bytes: &'a [u8],
) -> Result<(T, DecoderBuffer<'a>), Error> {
    let (actual_value, remaining) = DecoderBuffer::new(expected_bytes).decode()?;
    Ok((actual_value, remaining))
}

pub fn ensure_decoding_matches<'a, T: DecoderValue<'a> + PartialEq + core::fmt::Debug>(
    expected_value: &T,
    expected_bytes: &'a [u8],
) -> Result<(), Error> {
    let (actual_value, remaining) = decode(expected_bytes)?;
    ensure!(
        expected_value == &actual_value,
        "mut decodings do not match",
    );
    remaining.ensure_empty()?;
    Ok(())
}

pub fn decode_mut<'a, T: DecoderValueMut<'a>>(
    expected_bytes: &'a mut [u8],
) -> Result<(T, DecoderBufferMut<'a>), Error> {
    let (actual_value, remaining) = DecoderBufferMut::new(expected_bytes).decode()?;
    Ok((actual_value, remaining))
}

pub fn ensure_decoding_mut_matches<'a, T: DecoderValueMut<'a> + PartialEq + core::fmt::Debug>(
    expected_value: &T,
    expected_bytes: &'a mut [u8],
) -> Result<(), Error> {
    let (actual_value, remaining) = decode_mut(expected_bytes)?;
    ensure!(
        expected_value == &actual_value,
        "mut decodings do not match",
    );
    remaining.ensure_empty()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{i24, i48, u24, u48};
    use bolero::check;

    #[test]
    fn u8_round_trip_value_test() {
        for i in 0..=core::u8::MAX {
            ensure_codec_round_trip_value!(u8, i).unwrap();
        }
    }

    #[test]
    fn u8_round_trip_bytes_test() {
        let bytes = (0..=core::u8::MAX).collect::<Vec<_>>();
        ensure_codec_round_trip_bytes!(u8, &bytes).unwrap();
    }

    #[test]
    fn u16_round_trip_value_test() {
        for i in 0..=u16::MAX {
            ensure_codec_round_trip_value!(u16, i).unwrap();
        }
    }

    #[test]
    fn u16_round_trip_bytes_test() {
        let mut bytes = vec![];
        for v in 0..=u16::MAX {
            bytes.extend_from_slice(&v.to_be_bytes());
        }
        ensure_codec_round_trip_bytes!(u16, &bytes).unwrap();
    }

    macro_rules! fuzz_tests {
        ($ty:ident, $value_test:ident, $bytes_test:ident) => {
            #[test]
            #[cfg_attr(kani, kani::proof, kani::solver(cadical))]
            fn $value_test() {
                check!().with_type().cloned().for_each(|v| {
                    ensure_codec_round_trip_value!($ty, v).unwrap();
                    let bytes = v.to_be_bytes();
                    let actual = $ty::from_be_bytes(bytes);
                    assert_eq!(v, actual);
                });
            }

            #[test]
            fn $bytes_test() {
                check!().for_each(|bytes| {
                    let _ = ensure_codec_round_trip_bytes!($ty, &bytes);
                });
            }
        };
    }

    fuzz_tests!(u24, u24_round_trip_value_test, u24_round_trip_bytes_test);
    fuzz_tests!(i24, i24_round_trip_value_test, i24_round_trip_bytes_test);
    fuzz_tests!(u32, u32_round_trip_value_test, u32_round_trip_bytes_test);
    fuzz_tests!(i32, i32_round_trip_value_test, i32_round_trip_bytes_test);
    fuzz_tests!(u48, u48_round_trip_value_test, u48_round_trip_bytes_test);
    fuzz_tests!(i48, i48_round_trip_value_test, i48_round_trip_bytes_test);
    fuzz_tests!(u64, u64_round_trip_value_test, u64_round_trip_bytes_test);
    fuzz_tests!(i64, i64_round_trip_value_test, i64_round_trip_bytes_test);
}
