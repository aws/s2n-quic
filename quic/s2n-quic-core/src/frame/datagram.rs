// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{frame::Tag, varint::VarInt};

use s2n_codec::{
    decoder_parameterized_value, DecoderBuffer, DecoderBufferMut, Encoder, EncoderValue,
};

//= https://www.rfc-editor.org/rfc/rfc9221#section-4
//# DATAGRAM frames are used to transmit application data in an
//# unreliable manner.  The Type field in the DATAGRAM frame takes the
//# form 0b0011000X (or the values 0x30 and 0x31).

macro_rules! datagram_tag {
    () => {
        0x30u8..=0x31u8
    };
}

const DATAGRAM_TAG: u8 = 0x30;
//= https://www.rfc-editor.org/rfc/rfc9221#section-4
//# The least significant bit of the Type field in the DATAGRAM frame is
//# the LEN bit (0x01), which indicates whether there is a Length field
//# present: if this bit is set to 0, the Length field is absent and the
//# Datagram Data field extends to the end of the packet; if this bit is
//# set to 1, the Length field is present.

const LEN_BIT: u8 = 0x01;

//= https://www.rfc-editor.org/rfc/rfc9221#section-4
//# DATAGRAM Frame {
//#   Type (i) = 0x30..0x31,
//#   [Length (i)],
//#   Datagram Data (..),
//# }

//= https://www.rfc-editor.org/rfc/rfc9221#section-4
//# DATAGRAM frames contain the following fields:
//#
//# Length:  A variable-length integer specifying the length of the
//#    Datagram Data field in bytes.  This field is present only when the
//#    LEN bit is set to 1.  When the LEN bit is set to 0, the Datagram
//#    Data field extends to the end of the QUIC packet.  Note that empty
//#    (i.e., zero-length) datagrams are allowed.
//#
//# Datagram Data:  The bytes of the datagram to be delivered.

pub type DatagramRef<'a> = Datagram<&'a [u8]>;
pub type DatagramMut<'a> = Datagram<&'a mut [u8]>;

#[derive(Debug, PartialEq, Eq)]
pub struct Datagram<Data> {
    /// If true, the frame is the last frame in the payload
    pub is_last_frame: bool,

    /// The bytes to be delivered.
    pub data: Data,
}

impl<Data> Datagram<Data> {
    #[inline]
    pub fn tag(&self) -> u8 {
        let mut tag: u8 = DATAGRAM_TAG;

        if !self.is_last_frame {
            tag |= LEN_BIT;
        }

        tag
    }

    /// Converts the datagram data from one type to another
    pub fn map_data<F: FnOnce(Data) -> Out, Out>(self, map: F) -> Datagram<Out> {
        Datagram {
            is_last_frame: self.is_last_frame,
            data: map(self.data),
        }
    }
}

decoder_parameterized_value!(
    impl<'a, Data> Datagram<Data> {
        fn decode(tag: Tag, buffer: Buffer) -> Result<Self> {
            let is_last_frame = tag & LEN_BIT != LEN_BIT;

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

            let frame = Datagram {
                is_last_frame,
                data,
            };

            Ok((frame, buffer))
        }
    }
);

impl<Data: EncoderValue> EncoderValue for Datagram<Data> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());

        if self.is_last_frame {
            buffer.encode(&self.data);
        } else {
            buffer.encode_with_len_prefix::<VarInt, _>(&self.data);
        }
    }
}

impl<'a> From<Datagram<DecoderBuffer<'a>>> for DatagramRef<'a> {
    #[inline]
    fn from(d: Datagram<DecoderBuffer<'a>>) -> Self {
        d.map_data(|data| data.into_less_safe_slice())
    }
}

impl<'a> From<Datagram<DecoderBufferMut<'a>>> for DatagramRef<'a> {
    #[inline]
    fn from(d: Datagram<DecoderBufferMut<'a>>) -> Self {
        d.map_data(|data| &*data.into_less_safe_slice())
    }
}

impl<'a> From<Datagram<DecoderBufferMut<'a>>> for DatagramMut<'a> {
    #[inline]
    fn from(d: Datagram<DecoderBufferMut<'a>>) -> Self {
        d.map_data(|data| data.into_less_safe_slice())
    }
}
