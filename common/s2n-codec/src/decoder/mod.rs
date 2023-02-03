// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

macro_rules! doc_comment {
    ($($x:expr)*; $($tt:tt)*) => {
        doc_comment!(@doc concat!($($x, "\n",)*), $($tt)*);
    };
    (@doc $x:expr, $($tt:tt)*) => {
        #[doc = $x]
        $($tt)*
    };
}

macro_rules! impl_buffer {
    ($name:ident, $result:ident, $value:ident, $value_call:ident, $parameterized:ident, $parameterized_call:ident, $split:ident) => {
        impl<'a> $name<'a> {
            doc_comment! {
                "Decode a slice of bytes by `count`, removing the slice from the current buffer"

                "```"
                "# use s2n_codec::*;"
                "let mut data = [0, 1, 2, 3, 4];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "let (slice, buffer) = buffer.decode_slice(5).unwrap();"
                "assert_eq!(slice, [0u8, 1, 2, 3, 4][..]);"
                ""
                "assert!(buffer.is_empty());"
                "```";

                #[inline]
                pub fn decode_slice(self, count: usize) -> $result<'a, $name<'a>> {
                    self.ensure_len(count)?;

                    let (slice, remaining) = self.bytes.$split(count);

                    Ok((Self::new(slice), Self::new(remaining)))
                }
            }

            doc_comment! {
                "Decode a value of type `T`, splitting the data from the current buffer"

                "```"
                "# use s2n_codec::*;"
                "let mut data = [0, 1, 2, 3, 4, 5, 6];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "let (value, buffer) = buffer.decode::<u8>().unwrap();"
                "assert_eq!(value, 0);"
                ""
                "let (value, buffer) = buffer.decode::<u16>().unwrap();"
                "assert_eq!(value, 258);"
                ""
                "let (value, buffer) = buffer.decode::<u32>().unwrap();"
                "assert_eq!(value, 50_595_078);"
                ""
                "assert!(buffer.is_empty());"
                "```";

                #[inline]
                pub fn decode<T: $value<'a>>(self) -> $result<'a, T> {
                    T::$value_call(self)
                }
            }

            doc_comment! {
                "Decode a slice prefixed by type `Length`, splitting the data from the"
                "current buffer."

                "With a `Length` as encoded `u8`:"
                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [5, 0, 1, 2, 3, 4];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                "let (slice, buffer) = buffer.decode_slice_with_len_prefix::<u8>().unwrap();"
                "assert_eq!(slice, [0u8, 1, 2, 3, 4][..]);"
                "assert!(buffer.is_empty())"
                "```"

                "With a `Length` as encoded `u16`:"
                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [0, 5, 0, 1, 2, 3, 4];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                "let (slice, buffer) = buffer.decode_slice_with_len_prefix::<u16>().unwrap();"
                "assert_eq!(slice, [0u8, 1, 2, 3, 4][..]);"
                "assert!(buffer.is_empty())"
                "```";

                #[inline]
                pub fn decode_slice_with_len_prefix<Length: $value<'a> + core::convert::TryInto<usize>>(
                    self,
                ) -> $result<'a, Self> {
                    let (len, buffer) = self.decode::<Length>()?;
                    let len = len.try_into().map_err(|_| DecoderError::LengthCapacityExceeded)?;
                    buffer.decode_slice(len)
                }
            }

            doc_comment! {
                "Decode a value of type `T` prefixed by type `Length`, splitting the data from the"
                "current buffer."

                "With a `Length` as encoded `u8` and `T` as `u16`:"
                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [2, 0, 1, 2, 3];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                "let (value, buffer) = buffer.decode_with_len_prefix::<u8, u16>().unwrap();"
                "assert_eq!(value, 1);"
                "assert_eq!(buffer, [2, 3][..])"
                "```"

                concat!("The `", stringify!($value) ,"` implementation of `T` must consume the entire subslice")
                "otherwise an error will be returned."

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [3, 0, 1, 2];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                "let result = buffer.decode_with_len_prefix::<u8, u16>();"
                "assert!(result.is_err())"
                "```";

                #[inline]
                pub fn decode_with_len_prefix<Length: $value<'a> + core::convert::TryInto<usize>, T: $value<'a>>(
                    self,
                ) -> $result<'a, T> {
                    let (slice, buffer) = self.decode_slice_with_len_prefix::<Length>()?;
                    let (value, slice) = slice.decode::<T>()?;
                    slice.ensure_empty()?;
                    Ok((value, buffer))
                }
            }

            doc_comment! {
                "Decode a parameterized value of type `T` implementing `"
                stringify!($parameterized) "`";

                #[inline]
                pub fn decode_parameterized<T: $parameterized<'a>>(
                    self,
                    parameter: T::Parameter,
                ) -> $result<'a, T> {
                    T::$parameterized_call(parameter, self)
                }
            }

            doc_comment! {
                "Skip a `count` of bytes, discarding the bytes"

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [0, 1, 2, 3, 4];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                "let buffer = buffer.skip(3).unwrap();"
                "assert_eq!(buffer, [3, 4][..]);"
                "```";

                #[inline]
                pub fn skip(self, count: usize) -> Result<$name<'a>, DecoderError> {
                    self.decode_slice(count).map(|(_, buffer)| buffer)
                }
            }

            doc_comment! {
                "Skip a number of bytes encoded as a length prefix of type `Length`"

                "With a `Length` encoded as `u8`:"
                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [5, 0, 1, 2, 3, 4];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                "let buffer = buffer.skip_with_len_prefix::<u8>().unwrap();"
                "assert!(buffer.is_empty());"
                "```";

                #[inline]
                pub fn skip_with_len_prefix<Length: $value<'a> + core::convert::TryInto<usize>>(
                    self,
                ) -> Result<$name<'a>, DecoderError> {
                    let (len, buffer) = self.decode::<Length>()?;
                    let len = len.try_into().map_err(|_| DecoderError::LengthCapacityExceeded)?;
                    buffer.skip(len)
                }
            }

            doc_comment! {
                "Skip a `count` of bytes, returning a `CheckedRange` for later access";
                // TODO add doctest

                #[inline]
                pub fn skip_into_range(
                    self,
                    count: usize,
                    original_buffer: &crate::DecoderBufferMut,
                ) -> $result<'a, crate::CheckedRange> {
                    let start = original_buffer.len() - self.len();
                    let (slice, buffer) = self.decode_slice(count)?;
                    let end = start + count;
                    Ok((
                        crate::CheckedRange::new(start, end, slice.bytes.as_ptr()),
                        buffer,
                    ))
                }
            }

            doc_comment! {
                "Skip a number of bytes encoded as a length prefix of type `Length`"
                "into a `CheckedRange` for later access";
                // TODO add doctest

                #[inline]
                pub fn skip_into_range_with_len_prefix<Length: $value<'a> + core::convert::TryInto<usize>>(
                    self,
                    original_buffer: &crate::DecoderBufferMut,
                ) -> $result<'a, crate::CheckedRange> {
                    let (len, buffer) = self.decode::<Length>()?;
                    let len = len.try_into().map_err(|_| DecoderError::LengthCapacityExceeded)?;
                    buffer.skip_into_range(len, original_buffer)
                }
            }

            doc_comment! {
                "Reads data from a `CheckedRange`";
                // TODO add doctest

                #[inline]
                pub fn get_checked_range(&self, range: &crate::CheckedRange) -> DecoderBuffer {
                    range.get(self.bytes).into()
                }
            }

            doc_comment! {
                "Create a peeking `DecoderBuffer` from the current buffer view"

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [0, 1];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "let peek = buffer.peek();"
                "let (value, peek) = peek.decode::<u16>().unwrap();"
                "assert_eq!(value, 1);"
                "assert!(peek.is_empty());"
                ""
                "// `buffer` still contains the previous view"
                "assert_eq!(buffer, [0, 1][..]);"
                "```";

                #[inline]
                #[must_use]
                pub fn peek(&'a self) -> crate::DecoderBuffer<'a> {
                    crate::DecoderBuffer::new(self.bytes)
                }
            }

            doc_comment! {
                "Returns a single byte at `index`"

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [0, 1, 2];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "assert_eq!(buffer.peek_byte(0).unwrap(), 0);"
                "assert_eq!(buffer.peek_byte(1).unwrap(), 1);"
                "assert_eq!(buffer.peek_byte(2).unwrap(), 2);"
                ""
                "// `buffer` has not changed"
                "assert_eq!(buffer, [0, 1, 2][..]);"
                "```";

                #[inline]
                pub fn peek_byte(&self, index: usize) -> Result<u8, DecoderError> {
                    self.bytes
                        .get(index)
                        .cloned()
                        .ok_or_else(|| DecoderError::UnexpectedEof(index))
                }
            }

            /// Returns a `PeekBuffer` by `range`
            #[inline]
            pub fn peek_range(
                &self,
                range: core::ops::Range<usize>,
            ) -> Result<crate::DecoderBuffer, DecoderError> {
                let end = range.end;
                self.bytes
                    .get(range)
                    .map(|bytes| bytes.into())
                    .ok_or_else(|| DecoderError::UnexpectedEof(end))
            }

            doc_comment! {
                "Returns an error if the buffer is not empty."

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [1];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "assert!(buffer.ensure_empty().is_err());"
                "let (_, buffer) = buffer.decode::<u8>().unwrap();"
                "assert!(buffer.ensure_empty().is_ok());"
                "```";

                #[inline]
                pub fn ensure_empty(&self) -> Result<(), DecoderError> {
                    if !self.is_empty() {
                        Err(DecoderError::UnexpectedBytes(self.len()))
                    } else {
                        Ok(())
                    }
                }
            }

            doc_comment! {
                "Returns an error if the buffer does not have at least `len` bytes."

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [0, 1, 2];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "assert!(buffer.ensure_len(2).is_ok());"
                "assert!(buffer.ensure_len(5).is_err());"
                "```";

                #[inline]
                pub fn ensure_len(&self, len: usize) -> Result<(), DecoderError> {
                    if self.len() < len {
                        Err(DecoderError::UnexpectedEof(len))
                    } else {
                        Ok(())
                    }
                }
            }

            doc_comment! {
                "Returns the number of bytes in the buffer."

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [0, 1, 2];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "assert_eq!(buffer.len(), 3);"
                "let (_, buffer) = buffer.decode::<u8>().unwrap();"
                "assert_eq!(buffer.len(), 2);"
                "```";

                #[inline]
                pub fn len(&self) -> usize {
                    self.bytes.len()
                }
            }

            doc_comment! {
                "Returns true if the buffer has a length of 0."

                "```rust"
                "# use s2n_codec::*;"
                "let mut data = [1];"
                concat!("let buffer = ", stringify!($name), "::new(&mut data);")
                ""
                "assert!(!buffer.is_empty());"
                "let (_, buffer) = buffer.decode::<u8>().unwrap();"
                "assert!(buffer.is_empty());"
                "```";

                #[inline]
                pub fn is_empty(&self) -> bool {
                    self.bytes.is_empty()
                }
            }

            /// Borrows the buffer's slice. This should be used with caution, as it
            /// removes any panic protection this struct provides.
            #[inline]
            pub fn as_less_safe_slice(&'a self) -> &'a [u8] {
                self.bytes
            }
        }

        impl<'a> From<&'a mut [u8]> for $name<'a> {
            #[inline]
            fn from(bytes: &'a mut [u8]) -> Self {
                Self::new(bytes)
            }
        }

        impl<'a> PartialEq<[u8]> for $name<'a> {
            #[inline]
            fn eq(&self, rhs: &[u8]) -> bool {
                let bytes: &[u8] = self.bytes.as_ref();
                bytes.eq(rhs)
            }
        }
    };
}

pub mod buffer;
pub mod buffer_mut;
pub mod checked_range;
#[macro_use]
pub mod value;

pub use buffer::*;
pub use buffer_mut::*;
pub use checked_range::*;
pub use value::*;

#[derive(Debug)]
pub enum DecoderError {
    UnexpectedEof(usize),
    UnexpectedBytes(usize),
    LengthCapacityExceeded,
    InvariantViolation(&'static str), // TODO replace with a &'static Invariant
}

use core::fmt;
impl fmt::Display for DecoderError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // https://github.com/model-checking/kani/issues/1767#issuecomment-1275449305
        if cfg!(kani) {
            return Ok(());
        }

        match self {
            Self::UnexpectedEof(len) => write!(f, "unexpected eof: {len}"),
            Self::UnexpectedBytes(len) => write!(f, "unexpected bytes: {len}"),
            Self::LengthCapacityExceeded => write!(
                f,
                "length could not be represented in platform's usize type"
            ),
            Self::InvariantViolation(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<DecoderError> for &'static str {
    fn from(error: DecoderError) -> Self {
        match error {
            DecoderError::UnexpectedEof(_len) => "unexpected eof",
            DecoderError::UnexpectedBytes(_len) => "unexpected bytes",
            DecoderError::LengthCapacityExceeded => {
                "length could not be represented in platform's usize type"
            }
            DecoderError::InvariantViolation(msg) => msg,
        }
    }
}

#[macro_export]
macro_rules! decoder_invariant {
    ($expr:expr, $invariant:expr) => {
        if !($expr) {
            return ::core::result::Result::Err(
                $crate::decoder::DecoderError::InvariantViolation($invariant).into(),
            );
        }
    };
}
