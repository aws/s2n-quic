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

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.8
//# STREAM frames implicitly create a stream and carry stream data.  The
//# Type field in the STREAM frame takes the form 0b00001XXX (or the set
//# of values from 0x08 to 0x0f).

macro_rules! stream_tag {
    () => {
        0x08u8..=0x0fu8
    };
}

const STREAM_TAG: u8 = 0x08;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.8
//# *  The OFF bit (0x04) in the frame type is set to indicate that there
//#    is an Offset field present.  When set to 1, the Offset field is
//#    present.  When set to 0, the Offset field is absent and the Stream
//#    Data starts at an offset of 0 (that is, the frame contains the
//#    first bytes of the stream, or the end of a stream that includes no
//#    data).

const OFF_BIT: u8 = 0x04;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.8
//# *  The LEN bit (0x02) in the frame type is set to indicate that there
//#    is a Length field present.  If this bit is set to 0, the Length
//#    field is absent and the Stream Data field extends to the end of
//#    the packet.  If this bit is set to 1, the Length field is present.

const LEN_BIT: u8 = 0x02;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.8
//# *  The FIN bit (0x01) indicates that the frame marks the end of the
//#    stream.  The final size of the stream is the sum of the offset and
//#    the length of this frame.

const FIN_BIT: u8 = 0x01;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.8
//# STREAM Frame {
//#   Type (i) = 0x08..0x0f,
//#   Stream ID (i),
//#   [Offset (i)],
//#   [Length (i)],
//#   Stream Data (..),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.8
//# STREAM frames contain the following fields:
//#
//# Stream ID:  A variable-length integer indicating the stream ID of the
//#    stream; see Section 2.1.
//#
//# Offset:  A variable-length integer specifying the byte offset in the
//#    stream for the data in this STREAM frame.  This field is present
//#    when the OFF bit is set to 1.  When the Offset field is absent,
//#    the offset is 0.
//#
//# Length:  A variable-length integer specifying the length of the
//#    Stream Data field in this STREAM frame.  This field is present
//#    when the LEN bit is set to 1.  When the LEN bit is set to 0, the
//#    Stream Data field consumes all the remaining bytes in the packet.
//#
//# Stream Data:  The bytes from the designated stream to be delivered.

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
    #[inline]
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

    /// Converts the stream data from one type to another
    #[inline]
    pub fn map_data<F: FnOnce(Data) -> Out, Out>(self, map: F) -> Stream<Out> {
        Stream {
            stream_id: self.stream_id,
            offset: self.offset,
            is_last_frame: self.is_last_frame,
            is_fin: self.is_fin,
            data: map(self.data),
        }
    }
}

impl<Data: EncoderValue> Stream<Data> {
    /// Tries to fit the frame into the provided capacity
    ///
    /// The `is_last_frame` field will be updated with this call.
    ///
    /// If ok, the new payload length is returned, otherwise the frame cannot
    /// fit.
    #[inline]
    pub fn try_fit(&mut self, capacity: usize) -> Result<usize, FitError> {
        let mut fixed_len = 0;
        fixed_len += size_of::<Tag>();
        fixed_len += self.stream_id.encoding_size();

        if self.offset != 0u64 {
            fixed_len += self.offset.encoding_size();
        }

        let remaining_capacity = capacity.checked_sub(fixed_len).ok_or(FitError)?;

        let data_len = self.data.encoding_size();
        let max_data_len = remaining_capacity.min(data_len);

        // If data fits exactly into the capacity, mark it as the last frame
        if max_data_len == remaining_capacity {
            self.is_last_frame = true;
            return Ok(max_data_len);
        }

        self.is_last_frame = false;

        // Compute the maximum length prefix size we would need
        let len_prefix_size = VarInt::try_from(max_data_len)
            .map_err(|_| FitError)?
            .encoding_size();

        // Subtract the maximum length prefix size from the remaining capacity
        //
        // NOTE: It's possible that this result isn't completely optimal in every case. However,
        //       instead of spending extra cycles fitting a couple of bytes into the frame, it's
        //       good enough in most cases.
        let prefixed_data_len = remaining_capacity
            .checked_sub(len_prefix_size)
            .ok_or(FitError)?;

        let data_len = prefixed_data_len.min(data_len);

        Ok(data_len)
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
    #[inline]
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

    /// We hand optimize this encoding size so we can quickly estimate
    /// how large a STREAM frame will be
    #[inline]
    fn encoding_size_for_encoder<E: Encoder>(&self, encoder: &E) -> usize {
        let mut len = 0;
        len += size_of::<Tag>();
        len += self.stream_id.encoding_size();

        if *self.offset != 0 {
            len += self.offset.encoding_size();
        }

        let data_len = self.data.encoding_size_for_encoder(encoder);
        len += data_len;

        // include the len prefix
        if !self.is_last_frame {
            len += VarInt::try_from(data_len).unwrap().encoding_size();
        }

        // make sure the encoding size matches what we would actually encode
        if cfg!(debug_assertions) {
            use s2n_codec::EncoderLenEstimator;

            let mut estimator = EncoderLenEstimator::new(encoder.remaining_capacity());
            self.encode(&mut estimator);
            assert_eq!(estimator.len(), len);
        }

        len
    }
}

impl<'a> From<Stream<DecoderBuffer<'a>>> for StreamRef<'a> {
    #[inline]
    fn from(s: Stream<DecoderBuffer<'a>>) -> Self {
        s.map_data(|data| data.into_less_safe_slice())
    }
}

impl<'a> From<Stream<DecoderBufferMut<'a>>> for StreamRef<'a> {
    #[inline]
    fn from(s: Stream<DecoderBufferMut<'a>>) -> Self {
        s.map_data(|data| &*data.into_less_safe_slice())
    }
}

impl<'a> From<Stream<DecoderBufferMut<'a>>> for StreamMut<'a> {
    #[inline]
    fn from(s: Stream<DecoderBufferMut<'a>>) -> Self {
        s.map_data(|data| data.into_less_safe_slice())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::Padding;
    use bolero::check;
    use core::convert::TryInto;

    fn model(stream_id: VarInt, offset: VarInt, length: VarInt, capacity: usize) {
        let length = if let Ok(length) = VarInt::try_into(length) {
            length
        } else {
            // if the length cannot be represented by `usize` then bail
            return;
        };

        let mut frame = Stream {
            stream_id,
            offset,
            is_last_frame: false,
            is_fin: false,
            data: Padding { length },
        };

        if let Ok(new_length) = frame.try_fit(capacity) {
            frame.data = Padding { length: new_length };

            // we should never exceed the capacity
            assert!(
                frame.encoding_size() <= capacity,
                "the encoding_size should not exceed capacity {frame:#?}"
            );

            if new_length < length {
                let mut min = capacity;

                // allow the payload to be smaller by the encoding size of the length prefix
                if !frame.is_last_frame {
                    min -= VarInt::try_from(new_length).unwrap().encoding_size();
                }

                // the payload was trimmed so we should be at capacity
                let max = capacity;

                assert!(
                    (min..=max).contains(&frame.encoding_size()),
                    "encoding_size ({}) should match capacity ({capacity}) {frame:#?}",
                    frame.encoding_size(),
                );
            }

            if frame.is_last_frame {
                // the `is_last_frame` should _only_ be set when the encoding size == capacity
                assert_eq!(
                    frame.encoding_size(),
                    capacity,
                    "should only be the last frame if == capacity {frame:#?}"
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
            .for_each(|(stream_id, offset, length, capacity)| {
                model(stream_id, offset, length, capacity);
            });
    }
}
