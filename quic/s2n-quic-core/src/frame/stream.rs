use crate::{
    frame::{MaxPayloadSizeForFrame, Tag},
    varint::VarInt,
};
use core::mem::size_of;
use s2n_codec::{
    decoder_parameterized_value, DecoderBuffer, DecoderBufferMut, Encoder, EncoderValue,
};

//=https://quicwg.org/base-drafts/draft-ietf-quic-transport.html#rfc.section.19.8
//# 19.8.  STREAM Frames
//#
//#    STREAM frames implicitly create a stream and carry stream data.  The
//#    STREAM frame takes the form 0b00001XXX (or the set of values from
//#    0x08 to 0x0f).  The value of the three low-order bits of the frame
//#    type determines the fields that are present in the frame.

macro_rules! stream_tag {
    () => {
        0x08u8..=0x0fu8
    };
}

const STREAM_TAG: u8 = 0x08;

//#    o  The OFF bit (0x04) in the frame type is set to indicate that there
//#       is an Offset field present.  When set to 1, the Offset field is
//#       present.  When set to 0, the Offset field is absent and the Stream
//#       Data starts at an offset of 0 (that is, the frame contains the
//#       first bytes of the stream, or the end of a stream that includes no
//#       data).

const OFF_BIT: u8 = 0x04;

//#    o  The LEN bit (0x02) in the frame type is set to indicate that there
//#       is a Length field present.  If this bit is set to 0, the Length
//#       field is absent and the Stream Data field extends to the end of
//#       the packet.  If this bit is set to 1, the Length field is present.

const LEN_BIT: u8 = 0x02;

//#    o  The FIN bit (0x01) of the frame type is set only on frames that
//#       contain the final size of the stream.  Setting this bit indicates
//#       that the frame marks the end of the stream.

const FIN_BIT: u8 = 0x01;

//#    An endpoint that receives a STREAM frame for a send-only stream MUST
//#    terminate the connection with error STREAM_STATE_ERROR.
//#
//#    The STREAM frames are as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                         Stream ID (i)                       ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                         [Offset (i)]                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                         [Length (i)]                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Stream Data (*)                      ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#                       Figure 20: STREAM Frame Format
//#
//#    STREAM frames contain the following fields:
//#
//#    Stream ID:  A variable-length integer indicating the stream ID of the
//#       stream (see Section 2.1).
//#
//#    Offset:  A variable-length integer specifying the byte offset in the
//#       stream for the data in this STREAM frame.  This field is present
//#       when the OFF bit is set to 1.  When the Offset field is absent,
//#       the offset is 0.
//#
//#    Length:  A variable-length integer specifying the length of the
//#       Stream Data field in this STREAM frame.  This field is present
//#       when the LEN bit is set to 1.  When the LEN bit is set to 0, the
//#       Stream Data field consumes all the remaining bytes in the packet.
//#
//#    Stream Data:  The bytes from the designated stream to be delivered.
//#
//#    When a Stream Data field has a length of 0, the offset in the STREAM
//#    frame is the offset of the next byte that would be sent.
//#
//#    The first byte in the stream has an offset of 0.  The largest offset
//#    delivered on a stream - the sum of the offset and data length -
//#    cannot exceed 2^62-1, as it is not possible to provide flow control
//#    credit for that data.  Receipt of a frame that exceeds this limit
//#    will be treated as a connection error of type FLOW_CONTROL_ERROR.

#[derive(Debug, PartialEq, Eq)]
pub struct Stream<Data> {
    /// A variable-length integer indicating the stream ID of the stream
    pub stream_id: VarInt,

    /// A variable-length integer specifying the byte offset in the
    /// stream for the data in this STREAM frame.
    pub offset: VarInt,

    /// If true, the frame is the last frame in the payload
    pub is_last_frame: bool,

    /// If true, the frame marks the end of the stream.
    pub is_fin: bool,

    /// The bytes from the designated stream to be delivered.
    pub data: Data,
}

pub type StreamRef<'a> = Stream<&'a [u8]>;
pub type StreamMut<'a> = Stream<&'a mut [u8]>;

impl<Data> Stream<Data> {
    pub fn tag(&self) -> u8 {
        let mut tag: u8 = STREAM_TAG;

        if *self.offset != 0 {
            tag |= OFF_BIT;
        }

        if !self.is_last_frame {
            tag |= LEN_BIT;
        }

        if self.is_fin {
            tag |= FIN_BIT;
        }

        tag
    }

    /// Returns the maximum payload size a frame of a given size can carry
    pub fn max_payload_size(
        max_frame_size: usize,
        stream_id: VarInt,
        offset: VarInt,
    ) -> MaxPayloadSizeForFrame {
        let min_required_size = size_of::<Tag>()
            + stream_id.encoding_size()
            + if offset == VarInt::from_u8(0) {
                0
            } else {
                offset.encoding_size()
            };

        if min_required_size >= max_frame_size {
            return Default::default();
        }

        // If no length field gets added to the Frame, we have the following
        // available space. Otherwise there is less space available, depending
        // on the length of the length field.
        let max_payload_as_last_frame = max_frame_size - min_required_size;

        let max_payload_in_all_frames = if max_payload_as_last_frame > 4 {
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
            max_payload_as_last_frame - 4
        } else {
            0
        };

        MaxPayloadSizeForFrame {
            max_payload_as_last_frame,
            max_payload_in_all_frames,
        }
    }

    /// Returns an upper bound for the size of the frame that intends to
    /// store the given amount of data.
    ///
    /// The actual frame size might be lower, but is never allowed to be higher.
    pub fn get_max_frame_size(stream_id: VarInt, min_payload: usize) -> usize {
        size_of::<Tag>() + stream_id.encoding_size() +
        8 /* Offset size */ + 4 /* Size of len */ + min_payload
    }
}

decoder_parameterized_value!(
    impl<'a, Data> Stream<Data> {
        fn decode(tag: Tag, buffer: Buffer) -> Result<Self> {
            let has_offset = tag & OFF_BIT == OFF_BIT;
            let is_last_frame = tag & LEN_BIT != LEN_BIT;
            let is_fin = tag & FIN_BIT == FIN_BIT;

            let (stream_id, buffer) = buffer.decode()?;

            let (offset, buffer) = if has_offset {
                buffer.decode()?
            } else {
                (Default::default(), buffer)
            };

            let (data, buffer) = if !is_last_frame {
                let (data, buffer) = buffer.decode_with_len_prefix::<VarInt, Data>()?;
                (data, buffer)
            } else {
                let len = buffer.len();
                let (data, buffer) = buffer.decode_slice(len)?;
                let (data, remaining) = data.decode()?;
                remaining.ensure_empty()?;
                (data, buffer)
            };

            let frame = Stream {
                stream_id,
                offset,
                is_last_frame,
                is_fin,
                data,
            };

            Ok((frame, buffer))
        }
    }
);

impl<Data: EncoderValue> EncoderValue for Stream<Data> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.stream_id);

        if *self.offset != 0 {
            buffer.encode(&self.offset);
        }

        if self.is_last_frame {
            buffer.encode(&self.data);
        } else {
            buffer.encode_with_len_prefix::<VarInt, _>(&self.data);
        }
    }
}

impl<'a> Into<StreamRef<'a>> for Stream<DecoderBuffer<'a>> {
    fn into(self) -> StreamRef<'a> {
        Stream {
            stream_id: self.stream_id,
            offset: self.offset,
            is_last_frame: self.is_last_frame,
            is_fin: self.is_fin,
            data: self.data.into_less_safe_slice(),
        }
    }
}

impl<'a> Into<StreamRef<'a>> for Stream<DecoderBufferMut<'a>> {
    fn into(self) -> StreamRef<'a> {
        Stream {
            stream_id: self.stream_id,
            offset: self.offset,
            is_last_frame: self.is_last_frame,
            is_fin: self.is_fin,
            data: self.data.into_less_safe_slice(),
        }
    }
}

impl<'a> Into<StreamMut<'a>> for Stream<DecoderBufferMut<'a>> {
    fn into(self) -> StreamMut<'a> {
        Stream {
            stream_id: self.stream_id,
            offset: self.offset,
            is_last_frame: self.is_last_frame,
            is_fin: self.is_fin,
            data: self.data.into_less_safe_slice(),
        }
    }
}
