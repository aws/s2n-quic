use crate::{
    application::{ApplicationErrorCode, ApplicationErrorExt},
    frame::Tag,
    varint::VarInt,
};
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//=https://quicwg.org/base-drafts/draft-ietf-quic-transport.html#rfc.section.19.19
//# 19.19.  CONNECTION_CLOSE Frames
//#
//#    An endpoint sends a CONNECTION_CLOSE frame (type=0x1c or 0x1d) to
//#    notify its peer that the connection is being closed.  The
//#    CONNECTION_CLOSE with a frame type of 0x1c is used to signal errors
//#    at only the QUIC layer, or the absence of errors (with the NO_ERROR
//#    code).  The CONNECTION_CLOSE frame with a type of 0x1d is used to
//#    signal an error with the application that uses QUIC.

macro_rules! connection_close_tag {
    () => {
        0x1cu8..=0x1du8
    };
}
const QUIC_ERROR_TAG: u8 = 0x1c;
const APPLICATION_ERROR_TAG: u8 = 0x1d;

//#    If there are open streams that haven't been explicitly closed, they
//#    are implicitly closed when the connection is closed.
//#
//#    The CONNECTION_CLOSE frames are as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                         Error Code (i)                      ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                       [ Frame Type (i) ]                    ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                    Reason Phrase Length (i)                 ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Reason Phrase (*)                    ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#    CONNECTION_CLOSE frames contain the following fields:
//#
//#    Error Code:  A variable length integer error code which indicates the
//#       reason for closing this connection.  A CONNECTION_CLOSE frame of
//#       type 0x1c uses codes from the space defined in Section 20.  A
//#       CONNECTION_CLOSE frame of type 0x1d uses codes from the
//#       application protocol error code space; see Section 20.1
//#
//#    Frame Type:  A variable-length integer encoding the type of frame
//#       that triggered the error.  A value of 0 (equivalent to the mention
//#       of the PADDING frame) is used when the frame type is unknown.  The
//#       application-specific variant of CONNECTION_CLOSE (type 0x1d) does
//#       not include this field.
//#
//#    Reason Phrase Length:  A variable-length integer specifying the
//#       length of the reason phrase in bytes.  Because a CONNECTION_CLOSE
//#       frame cannot be split between packets, any limits on packet size
//#       will also limit the space available for a reason phrase.
//#
//#    Reason Phrase:  A human-readable explanation for why the connection
//#       was closed.  This can be zero length if the sender chooses to not
//#       give details beyond the Error Code.  This SHOULD be a UTF-8
//#       encoded string [RFC3629].

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

impl<'a> ConnectionClose<'a> {
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
impl<'a> ApplicationErrorExt for ConnectionClose<'a> {
    fn application_error_code(&self) -> Option<ApplicationErrorCode> {
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

impl<'a> EncoderValue for ConnectionClose<'a> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());

        if let Some(frame_type) = &self.frame_type {
            buffer.encode(&self.error_code);
            buffer.encode(frame_type);
        } else {
            buffer.encode(&self.error_code);
        }

        if let Some(reason) = &self.reason {
            buffer.encode_with_len_prefix::<VarInt, _>(reason);
        } else {
            buffer.encode(&0u8);
        }
    }
}
