// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    frame::{FitError, Tag},
    varint::VarInt,
};
use core::{convert::TryFrom, mem::size_of};
use s2n_codec::{
    decoder_parameterized_value, DecoderBuffer, DecoderBufferMut, Encoder, EncoderValue,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.6
//# A CRYPTO frame (type=0x06) is used to transmit cryptographic
//# handshake messages.

macro_rules! crypto_tag {
    () => {
        0x06u8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.6
//# CRYPTO Frame {
//#   Type (i) = 0x06,
//#   Offset (i),
//#   Length (i),
//#   Crypto Data (..),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.6
//# CRYPTO frames contain the following fields:
//#
//# Offset:  A variable-length integer specifying the byte offset in the
//#    stream for the data in this CRYPTO frame.
//#
//# Length:  A variable-length integer specifying the length of the
//#    Crypto Data field in this CRYPTO frame.
//#
//# Crypto Data:  The cryptographic message data.

#[derive(Debug, PartialEq, Eq)]
pub struct Crypto<Data> {
    /// A variable-length integer specifying the byte offset in the stream
    /// for the data in this CRYPTO frame.
    pub offset: VarInt,

    /// The cryptographic message data.
    pub data: Data,
}

impl<Data> Crypto<Data> {
    #[inline]
    pub const fn tag(&self) -> u8 {
        crypto_tag!()
    }

    /// Converts the crypto data from one type to another
    #[inline]
    pub fn map_data<F: FnOnce(Data) -> Out, Out>(self, map: F) -> Crypto<Out> {
        Crypto {
            offset: self.offset,
            data: map(self.data),
        }
    }
}

impl<Data: EncoderValue> Crypto<Data> {
    /// Tries to fit the frame into the provided capacity
    ///
    /// If ok, the new payload length is returned, otherwise the frame cannot
    /// fit.
    #[inline]
    pub fn try_fit(&self, capacity: usize) -> Result<usize, FitError> {
        let mut fixed_len = 0;
        fixed_len += size_of::<Tag>();
        fixed_len += self.offset.encoding_size();

        let remaining_capacity = capacity.checked_sub(fixed_len).ok_or(FitError)?;

        let data_len = self.data.encoding_size();
        let max_data_len = remaining_capacity.min(data_len);

        let len_prefix_size = VarInt::try_from(max_data_len)
            .map_err(|_| FitError)?
            .encoding_size();

        let prefixed_data_len = remaining_capacity
            .checked_sub(len_prefix_size)
            .ok_or(FitError)?;
        let data_len = prefixed_data_len.min(data_len);

        Ok(data_len)
    }
}

pub type CryptoRef<'a> = Crypto<&'a [u8]>;
pub type CryptoMut<'a> = Crypto<&'a mut [u8]>;

decoder_parameterized_value!(
    impl<'a, Data> Crypto<Data> {
        fn decode(_tag: Tag, buffer: Buffer) -> Result<Self> {
            let (offset, buffer) = buffer.decode()?;
            let (data, buffer) = buffer.decode_with_len_prefix::<VarInt, Data>()?;

            let frame = Crypto { offset, data };

            Ok((frame, buffer))
        }
    }
);

impl<Data: EncoderValue> EncoderValue for Crypto<Data> {
    #[inline]
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.offset);
        buffer.encode_with_len_prefix::<VarInt, _>(&self.data);
    }
}

impl<'a> From<Crypto<DecoderBuffer<'a>>> for CryptoRef<'a> {
    #[inline]
    fn from(s: Crypto<DecoderBuffer<'a>>) -> Self {
        s.map_data(|data| data.into_less_safe_slice())
    }
}

impl<'a> From<Crypto<DecoderBufferMut<'a>>> for CryptoRef<'a> {
    #[inline]
    fn from(s: Crypto<DecoderBufferMut<'a>>) -> Self {
        s.map_data(|data| &*data.into_less_safe_slice())
    }
}

impl<'a> From<Crypto<DecoderBufferMut<'a>>> for CryptoMut<'a> {
    #[inline]
    fn from(s: Crypto<DecoderBufferMut<'a>>) -> Self {
        s.map_data(|data| data.into_less_safe_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Padding;
    use bolero::check;
    use core::convert::TryInto;

    fn model(offset: VarInt, length: VarInt, capacity: usize) {
        let length = if let Ok(length) = VarInt::try_into(length) {
            length
        } else {
            // if the length cannot be represented by `usize` then bail
            return;
        };

        let mut frame = Crypto {
            offset,
            data: Padding { length },
        };

        if let Ok(new_length) = frame.try_fit(capacity) {
            frame.data = Padding { length: new_length };

            assert!(
                frame.encoding_size() <= capacity,
                "the encoding_size should not exceed capacity {frame:#?}"
            );

            if new_length < length {
                // Ideally `frame.encoding_size() == capacity` but in some cases, the payload
                // needs to be decreased to fit `capacity` and by decreasing the payload size,
                // the length prefix is also decreased.
                //
                // The tolerance is based on the length prefix encoding size.
                // For example, if the length prefix requires 2 bytes to encode the length,
                // the overall `frame.encoding_size()` can be within 2 bytes of `capacity`.
                let tolerance = VarInt::try_from(new_length).unwrap().encoding_size();

                assert!(
                    capacity - frame.encoding_size() <= tolerance,
                    "should fit capacity tolerance: expected {}, got {}; {:#?}",
                    tolerance,
                    capacity - frame.encoding_size(),
                    frame,
                );
            }
        } else {
            assert!(
                frame.encoding_size() > capacity,
                "rejection should only occur when encoding size > capacity {frame:#?}"
            );
        }
    }

    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(1), kani::solver(kissat))]
    fn try_fit_test() {
        check!()
            .with_type()
            .cloned()
            .for_each(|(offset, length, capacity)| {
                model(offset, length, capacity);
            });
    }
}
