use crate::frame::Tag;
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//=https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.1
//# 19.1.  PADDING Frame
//#
//#    The PADDING frame (type=0x00) has no semantic value.  PADDING frames
//#    can be used to increase the size of a packet.  Padding can be used to
//#    increase an initial client packet to the minimum required size, or to
//#    provide protection against traffic analysis for protected packets.

macro_rules! padding_tag {
    () => {
        0x00u8
    };
}

//#    A PADDING frame has no content. That is, a PADDING frame consists of
//#    the single byte that identifies the frame as a PADDING frame.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Padding {
    pub length: usize,
}

impl Padding {
    /// The maximum padding allowed. When placed at the end of the packet
    /// all of the remaining bytes will be consumed.
    pub const MAX: Self = Self {
        length: core::usize::MAX,
    };

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
        // Padding will flex based on the available bytes in the encoder
        let len = encoder.remaining_capacity().min(self.length);
        encoder.write_repeated(len, 0)
    }
}
