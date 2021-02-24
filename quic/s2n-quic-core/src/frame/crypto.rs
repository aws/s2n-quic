use crate::{
    frame::{MaxPayloadSizeForFrame, Tag},
    varint::VarInt,
};
use core::mem::size_of;
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

impl<Data> Crypto<Data> {
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

    /// Converts the stream data from one type to another
    pub fn map_data<F: FnOnce(Data) -> Out, Out>(self, map: F) -> Crypto<Out> {
        Crypto {
            offset: self.offset,
            data: map(self.data),
        }
    }
}

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
    use insta::assert_debug_snapshot;

    #[test]
    #[cfg_attr(miri, ignore)] // snapshot tests don't work on miri
    fn max_frame_size_snapshot() {
        assert_debug_snapshot!("max_frame_size_snapshot", CryptoRef::get_max_frame_size(16));
    }
}
