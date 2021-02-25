use crate::{
    frame::{FitError, MaxPayloadSizeForFrame, Tag},
    varint::VarInt,
};
use core::{convert::TryFrom, mem::size_of};
use s2n_codec::{
    decoder_parameterized_value, DecoderBuffer, DecoderBufferMut, Encoder, EncoderValue,
};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.6
//# A CRYPTO frame (type=0x06) is used to transmit cryptographic
//# handshake messages.

macro_rules! crypto_tag {
    () => {
        0x06u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.6
//# CRYPTO Frame {
//#   Type (i) = 0x06,
//#   Offset (i),
//#   Length (i),
//#   Crypto Data (..),
//# }

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.6
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
    pub const fn tag(&self) -> u8 {
        crypto_tag!()
    }

    /// Converts the stream data from one type to another
    pub fn map_data<F: FnOnce(Data) -> Out, Out>(self, map: F) -> Crypto<Out> {
        Crypto {
            offset: self.offset,
            data: map(self.data),
        }
    }

    /// Returns the maximum payload size a frame of a given size can carry
    pub fn max_payload_size(max_frame_size: usize, offset: VarInt) -> MaxPayloadSizeForFrame {
        // We use a maximum length field size of 4 here, since this will
        // cover up to 1GB of data. Due to other checks in the library we
        // will never exceed sending 1GB inside a single frame.
        // In the current state even 2byte for sending up to 16kB of data
        // would be sufficient, due to UDP packet size limitations. However
        // using 4 bytes will lave us prepared for using bigger packet sizes
        // in case hardware segmentation support is available in the future.
        //
        // The 4 byte assumption is a pessimistic estimate at this point,
        // since we do not know the actual data amount which will get written
        // to this frame. If it is below 64kB, we undererstimate the amount
        // of fitting data by 2 bytes. This might lead the implementation
        // to fragment the frame where it was otherwise not required in some
        // edge cases.
        // However since we do not necesarily know how much data to write
        // until we know how much space is available, the pessimistic
        // estimate is the best we can do at this point of time.
        const SIZE_LEN: usize = 4;

        let min_required_size = size_of::<Tag>() + offset.encoding_size() + SIZE_LEN;

        if min_required_size >= max_frame_size {
            // Can not store any data in the frame
            return Default::default();
        }

        let max_payload_size = max_frame_size - min_required_size;

        // Since CRYPTO frames do always require a length and offset fields, the
        // maximum size is the same independent of whether we store the frame as
        // the last frame in a packet or not.
        MaxPayloadSizeForFrame {
            max_payload_as_last_frame: max_payload_size,
            max_payload_in_all_frames: max_payload_size,
        }
    }

    /// Returns an upper bound for the size of the frame that intends to
    /// store the given amount of data.
    ///
    /// The actual frame size might be lower, but is never allowed to be higher.
    pub const fn get_max_frame_size(min_payload: usize) -> usize {
        size_of::<Tag>() +
        8 /* Offset size */ + 4 /* Size of len */ + min_payload
    }
}

impl<Data: EncoderValue> Crypto<Data> {
    /// Tries to fit the frame into the provided capacity
    ///
    /// If ok, the new payload length is returned, otherwise the frame cannot
    /// fit.
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
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.offset);
        buffer.encode_with_len_prefix::<VarInt, _>(&self.data);
    }
}

impl<'a> From<Crypto<DecoderBuffer<'a>>> for CryptoRef<'a> {
    fn from(s: Crypto<DecoderBuffer<'a>>) -> Self {
        s.map_data(|data| data.into_less_safe_slice())
    }
}

impl<'a> From<Crypto<DecoderBufferMut<'a>>> for CryptoRef<'a> {
    fn from(s: Crypto<DecoderBufferMut<'a>>) -> Self {
        s.map_data(|data| &data.into_less_safe_slice()[..])
    }
}

impl<'a> From<Crypto<DecoderBufferMut<'a>>> for CryptoMut<'a> {
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
            if new_length < length {
                // the payload was trimmed so we should be at full capacity
                assert_eq!(
                    frame.encoding_size(),
                    capacity,
                    "should match capacity {:#?}",
                    frame
                );
            } else {
                // we should never exceed the capacity
                assert!(
                    frame.encoding_size() <= capacity,
                    "the encoding_size should not exceed capacity {:#?}",
                    frame
                );
            }
        } else {
            assert!(
                frame.encoding_size() > capacity,
                "rejection should only occur when encoding size > capacity {:#?}",
                frame
            );
        }
    }

    #[test]
    fn try_fit_test() {
        check!()
            .with_type()
            .cloned()
            .for_each(|(offset, length, capacity)| {
                model(offset, length, capacity);
            });
    }
}
