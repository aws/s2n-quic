// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, frame::ExtensionTag, varint::VarInt};
use s2n_codec::{Encoder, EncoderValue};

const TAG: VarInt = VarInt::from_u32(0xdc0001);

macro_rules! mtu_probing_complete_tag {
    () => {
        0xdc0001u64
    };
}

//# MTU_PROBING_COMPLETE Frame {
//#   Type (i) = 0xdc0001,
//#   Mtu [16],
//# }

//# MTU_PROBING_COMPLETE frames contain the following fields:
//#
//# Mtu: A 16-bit unsigned integer indicating the maximum transmission
//#     unit that has been confirmed through probing.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MtuProbingComplete {
    /// A 16-bit value indicating the confirmed MTU
    pub mtu: u16,
}

impl MtuProbingComplete {
    pub const fn tag(&self) -> ExtensionTag {
        TAG
    }

    pub const fn new(mtu: u16) -> Self {
        Self { mtu }
    }
}

impl<'a> ::s2n_codec::DecoderParameterizedValue<'a> for MtuProbingComplete {
    type Parameter = ExtensionTag;

    #[inline]
    fn decode_parameterized(
        _tag: Self::Parameter,
        buffer: ::s2n_codec::DecoderBuffer<'a>,
    ) -> ::s2n_codec::DecoderBufferResult<'a, Self> {
        let (mtu, buffer) = buffer.decode::<u16>()?;
        let frame = MtuProbingComplete { mtu };
        Ok((frame, buffer))
    }
}

impl<'a> ::s2n_codec::DecoderParameterizedValueMut<'a> for MtuProbingComplete {
    type Parameter = ExtensionTag;

    #[inline]
    fn decode_parameterized_mut(
        _tag: Self::Parameter,
        buffer: ::s2n_codec::DecoderBufferMut<'a>,
    ) -> ::s2n_codec::DecoderBufferMutResult<'a, Self> {
        let (mtu, buffer) = buffer.decode::<u16>()?;
        let frame = MtuProbingComplete { mtu };
        Ok((frame, buffer))
    }
}

impl EncoderValue for MtuProbingComplete {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&TAG);
        buffer.encode(&self.mtu);
    }
}

impl event::IntoEvent<event::builder::Frame> for &MtuProbingComplete {
    #[inline]
    fn into_event(self) -> event::builder::Frame {
        event::builder::Frame::MtuProbingComplete {
            mtu: self.mtu,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_codec::{DecoderBuffer, DecoderParameterizedValue, EncoderValue};

    #[test]
    fn round_trip() {
        let frame = MtuProbingComplete::new(1500);
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);
        let (decoded_frame, remaining) =
            MtuProbingComplete::decode_parameterized(TAG, buffer).expect("decoding succeeds");
        assert!(remaining.is_empty());
        assert_eq!(frame.mtu, decoded_frame.mtu);
    }

    #[test]
    fn min_mtu() {
        let frame = MtuProbingComplete::new(0);
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);
        let (decoded_frame, _) =
            MtuProbingComplete::decode_parameterized(TAG, buffer).expect("decoding succeeds");
        assert_eq!(0, decoded_frame.mtu);
    }

    #[test]
    fn max_mtu() {
        let frame = MtuProbingComplete::new(u16::MAX);
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);
        let (decoded_frame, _) =
            MtuProbingComplete::decode_parameterized(TAG, buffer).expect("decoding succeeds");
        assert_eq!(u16::MAX, decoded_frame.mtu);
    }

    #[test]
    fn incomplete_frame() {
        let frame = MtuProbingComplete::new(1500);
        let mut encoded = frame.encode_to_vec();
        encoded.truncate(encoded.len() - 1);

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);

        assert!(MtuProbingComplete::decode_parameterized(TAG, buffer).is_err());
    }
}
