// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{application, frame::Tag, varint::VarInt};
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.19
//# An endpoint sends a CONNECTION_CLOSE frame (type=0x1c or 0x1d) to
//# notify its peer that the connection is being closed.  The
//# CONNECTION_CLOSE frame with a type of 0x1c is used to signal errors
//# at only the QUIC layer, or the absence of errors (with the NO_ERROR
//# code).  The CONNECTION_CLOSE frame with a type of 0x1d is used to
//# signal an error with the application that uses QUIC.

macro_rules! connection_close_tag {
    () => {
        0x1cu8..=0x1du8
    };
}
const QUIC_ERROR_TAG: u8 = 0x1c;
const APPLICATION_ERROR_TAG: u8 = 0x1d;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.19
//# CONNECTION_CLOSE Frame {
//#   Type (i) = 0x1c..0x1d,
//#   Error Code (i),
//#   [Frame Type (i)],
//#   Reason Phrase Length (i),
//#   Reason Phrase (..),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.19
//# CONNECTION_CLOSE frames contain the following fields:
//#
//# Error Code:  A variable-length integer that indicates the reason for
//# closing this connection.  A CONNECTION_CLOSE frame of type 0x1c
//# uses codes from the space defined in Section 20.1.  A
//# CONNECTION_CLOSE frame of type 0x1d uses codes defined by the
//# application protocol; see Section 20.2.
//#
//# Frame Type:  A variable-length integer encoding the type of frame
//# that triggered the error.  A value of 0 (equivalent to the mention
//# of the PADDING frame) is used when the frame type is unknown.  The
//# application-specific variant of CONNECTION_CLOSE (type 0x1d) does
//# not include this field.
//#
//# Reason Phrase Length:  A variable-length integer specifying the
//# length of the reason phrase in bytes.  Because a CONNECTION_CLOSE
//# frame cannot be split between packets, any limits on packet size
//# will also limit the space available for a reason phrase.
//#
//# Reason Phrase:  Additional diagnostic information for the closure.
//# This can be zero length if the sender chooses not to give details
//# beyond the Error Code value.  This SHOULD be a UTF-8 encoded
//# string [RFC3629], though the frame does not carry information,
//# such as language tags, that would aid comprehension by any entity
//# other than the one that created the text.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionClose<'a> {
    /// A variable length integer error code which indicates the reason
    /// for closing this connection.
    pub error_code: VarInt,

    /// A variable-length integer encoding the type of frame that
    /// triggered the error.
    pub frame_type: Option<VarInt>,

    /// A human-readable explanation for why the connection was closed.
    /// This SHOULD be a UTF-8 encoded string.
    pub reason: Option<&'a [u8]>,
}

impl ConnectionClose<'_> {
    #[inline]
    pub fn tag(&self) -> u8 {
        if self.frame_type.is_some() {
            QUIC_ERROR_TAG
        } else {
            APPLICATION_ERROR_TAG
        }
    }
}

// If a `ConnectionClose` contains no frame type it was sent by an application and contains
// an `ApplicationErrorCode`. Otherwise it is an error on the QUIC layer.
impl application::error::TryInto for ConnectionClose<'_> {
    #[inline]
    fn application_error(&self) -> Option<application::Error> {
        if self.frame_type.is_none() {
            Some(self.error_code.into())
        } else {
            None
        }
    }
}

decoder_parameterized_value!(
    impl<'a> ConnectionClose<'a> {
        fn decode(tag: Tag, buffer: Buffer) -> Result<Self> {
            let (error_code, buffer) = buffer.decode()?;

            let (frame_type, buffer) = if tag == QUIC_ERROR_TAG {
                let (frame_type, buffer) = buffer.decode()?;
                (Some(frame_type), buffer)
            } else {
                (None, buffer)
            };

            let (reason, buffer) = buffer.decode_slice_with_len_prefix::<VarInt>()?;

            let reason = if reason.is_empty() {
                None
            } else {
                // newer versions of clippy complain about redundant slicing
                // but we don't know if this is a `&slice` or `&mut slice`
                #[allow(clippy::all)]
                Some(&reason.into_less_safe_slice()[..])
            };

            let frame = ConnectionClose {
                error_code,
                frame_type,
                reason,
            };

            Ok((frame, buffer))
        }
    }
);

impl EncoderValue for ConnectionClose<'_> {
    #[inline]
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());

        buffer.encode(&self.error_code);
        if let Some(frame_type) = &self.frame_type {
            buffer.encode(frame_type);
        }

        if let Some(reason) = &self.reason {
            buffer.encode_with_len_prefix::<VarInt, _>(reason);
        } else {
            buffer.encode(&0u8);
        }
    }
}
