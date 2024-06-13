// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::frame::Tag;
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.1
//# A PADDING frame (type=0x00) has no semantic value.  PADDING frames
//# can be used to increase the size of a packet.  Padding can be used to
//# increase an Initial packet to the minimum required size or to provide
//# protection against traffic analysis for protected packets.

macro_rules! padding_tag {
    () => {
        0x00u8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.1
//# PADDING Frame {
//#   Type (i) = 0x00,
//# }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Padding {
    pub length: usize,
}

impl Padding {
    /// The maximum padding allowed. When placed at the end of the packet
    /// all of the remaining bytes will be consumed.
    pub const MAX: Self = Self { length: usize::MAX };

    pub const fn tag(self) -> u8 {
        padding_tag!()
    }
}

decoder_parameterized_value!(
    impl<'a> Padding {
        fn decode(_tag: Tag, buffer: Buffer) -> Result<Self> {
            let mut length = 0;
            while buffer
                .peek_byte(length)
                .map(|v| v == padding_tag!())
                .unwrap_or(false)
            {
                length += 1;
            }

            let buffer = buffer.skip(length).expect("padding already verified");

            // add one for tag itself - this needs to come after the skip, as the
            // tag has already been read.
            length += 1;

            let frame = Padding { length };

            Ok((frame, buffer))
        }
    }
);

impl EncoderValue for Padding {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        encoder.write_repeated(self.length, 0)
    }
}
