use crate::{
    frame::{MaxPayloadSizeForFrame, Tag},
    varint::VarInt,
};
use core::mem::size_of;
use s2n_codec::{
    decoder_parameterized_value, DecoderBuffer, DecoderBufferMut, Encoder, EncoderValue,
};

//=https://quicwg.org/base-drafts/draft-ietf-quic-transport.html#rfc.section.19.5
//# 19.6.  CRYPTO Frame
//#
//#    The CRYPTO frame (type=0x06) is used to transmit cryptographic
//#    handshake messages.  It can be sent in all packet types.  The CRYPTO
//#    frame offers the cryptographic protocol an in-order stream of bytes.
//#    CRYPTO frames are functionally identical to STREAM frames, except
//#    that they do not bear a stream identifier; they are not flow
//#    controlled; and they do not carry markers for optional offset,
//#    optional length, and the end of the stream.

macro_rules! crypto_tag {
    () => {
        0x06u8
    };
}

//#    The CRYPTO frame is as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                          Offset (i)                         ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                          Length (i)                         ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Crypto Data (*)                      ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#                       Figure 19: CRYPTO Frame Format
//#
//#    CRYPTO frames contain the following fields:
//#
//#    Offset:  A variable-length integer specifying the byte offset in the
//#       stream for the data in this CRYPTO frame.
//#
//#    Length:  A variable-length integer specifying the length of the
//#       Crypto Data field in this CRYPTO frame.
//#
//#    Crypto Data:  The cryptographic message data.
//#
//#    There is a separate flow of cryptographic handshake data in each
//#    encryption level, each of which starts at an offset of 0.  This
//#    implies that each encryption level is treated as a separate CRYPTO
//#    stream of data.
//#
//#    Unlike STREAM frames, which include a Stream ID indicating to which
//#    stream the data belongs, the CRYPTO frame carries data for a single
//#    stream per encryption level.  The stream does not have an explicit
//#    end, so CRYPTO frames do not have a FIN bit.

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
    pub fn get_max_frame_size(min_payload: usize) -> usize {
        size_of::<Tag>() +
        8 /* Offset size */ + 4 /* Size of len */ + min_payload
    }
}

impl<Data: EncoderValue> EncoderValue for Crypto<Data> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.offset);
        buffer.encode_with_len_prefix::<VarInt, _>(&self.data);
    }
}

impl<'a> Into<CryptoRef<'a>> for Crypto<DecoderBuffer<'a>> {
    fn into(self) -> CryptoRef<'a> {
        Crypto {
            offset: self.offset,
            data: self.data.into_less_safe_slice(),
        }
    }
}

impl<'a> Into<CryptoRef<'a>> for Crypto<DecoderBufferMut<'a>> {
    fn into(self) -> CryptoRef<'a> {
        Crypto {
            offset: self.offset,
            data: self.data.into_less_safe_slice(),
        }
    }
}

impl<'a> Into<CryptoMut<'a>> for Crypto<DecoderBufferMut<'a>> {
    fn into(self) -> CryptoMut<'a> {
        Crypto {
            offset: self.offset,
            data: self.data.into_less_safe_slice(),
        }
    }
}
