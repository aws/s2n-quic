use crate::decoder::{
    value::{DecoderParameterizedValue, DecoderValue},
    DecoderError,
};

pub type DecoderBufferResult<'a, T> = Result<(T, DecoderBuffer<'a>), DecoderError>;

/// DecoderBuffer is a panic-free byte buffer for look-ahead decoding untrusted input
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct DecoderBuffer<'a> {
    bytes: &'a [u8],
}

impl<'a> DecoderBuffer<'a> {
    /// Create a new `DecoderBuffer` from a byte slice
    #[inline]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes }
    }

    /// Move out the buffer's slice. This should be used with caution, as it
    /// removes any panic protection this struct provides.
    #[inline]
    pub fn into_less_safe_slice(self) -> &'a [u8] {
        self.bytes
    }
}

impl_buffer!(
    DecoderBuffer,
    DecoderBufferResult,
    DecoderValue,
    decode,
    DecoderParameterizedValue,
    decode_parameterized,
    split_at
);

impl<'a> From<&'a [u8]> for DecoderBuffer<'a> {
    #[inline]
    fn from(bytes: &'a [u8]) -> Self {
        Self::new(bytes)
    }
}
