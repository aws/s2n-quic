// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use crate::{
    event,
    frame::{ack_elicitation::AckElicitable, congestion_controlled::CongestionControlled},
};
use core::fmt;
use s2n_codec::{
    DecoderBuffer, DecoderBufferMut, DecoderBufferMutResult, DecoderError,
    DecoderParameterizedValueMut, DecoderValueMut, Encoder, EncoderValue,
};

pub mod ack_elicitation;
pub mod congestion_controlled;
pub mod path_validation;

#[cfg(test)]
mod tests;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19
//# As described in Section 12.4, packets contain one or more frames.
//# This section describes the format and semantics of the core QUIC
//# frame types.

pub(crate) type Tag = u8;

pub type FrameRef<'a> = Frame<'a, ack::AckRangesDecoder<'a>, DecoderBuffer<'a>>;
pub type FrameMut<'a> = Frame<'a, ack::AckRangesDecoder<'a>, DecoderBufferMut<'a>>;

pub trait FrameTrait: AckElicitable + CongestionControlled + path_validation::Probing {}

/// Implement FrameTrait for all types that implement the required subtraits
impl<T: AckElicitable + CongestionControlled + path_validation::Probing> FrameTrait for T {}

macro_rules! frames {
    ($ack:ident, $data:ident | $($tag_macro:ident => $module:ident, $handler:ident, $ty:ident $([$($generics:tt)+])?;)*) => {
        $(
            #[macro_use]
            pub mod $module;
            pub use $module::$ty;
        )*

        pub type RemainingBuffer<'a> = Option<DecoderBufferMut<'a>>;

        #[derive(Debug, PartialEq, Eq)]
        pub enum Frame<'a, $ack, $data> {
            $(
                $ty($module::$ty $(<$($generics)*>)?),
            )*
        }

        impl<'a, $ack, $data> Frame<'a, $ack, $data> {
            #[inline]
            pub fn tag(&self) -> Tag {
                match self {
                    $(
                        Frame::$ty(frame) => frame.tag(),
                    )*
                }
            }
        }

        impl<'a, $ack, $data> event::IntoEvent<event::builder::Frame> for &Frame<'a, $ack, $data>
        where
            $ack: crate::frame::ack::AckRanges,
            $data: EncoderValue,
        {
            #[inline]
            fn into_event(self) -> event::builder::Frame {
                match self {
                    $(
                        Frame::$ty(inner) => inner.into_event(),
                    )*
                }
            }
        }

        impl<'a, $ack, $data> ack_elicitation::AckElicitable for Frame<'a, $ack, $data> {
            #[inline]
            fn ack_elicitation(&self) -> ack_elicitation::AckElicitation {
                match self {
                    $(
                        Frame::$ty(frame) => frame.ack_elicitation(),
                    )*
                }
            }
        }

        impl<'a, $ack, $data> path_validation::Probing for Frame<'a, $ack, $data> {
            #[inline]
            fn path_validation(&self) -> path_validation::Probe {
                match self {
                    $(
                        Frame::$ty(frame) => frame.path_validation(),
                    )*
                }
            }
        }

        $(
            impl<'a, $ack, $data> From<$module::$ty $(<$($generics)*>)?> for Frame<'a, $ack, $data> {
                #[inline]
                fn from(v: $module::$ty $(<$($generics)*>)?)  -> Frame<'a, $ack, $data> {
                    Frame::$ty(v)
                }
            }
        )*

        impl<'a, $ack, $data: DecoderValueMut<'a>> DecoderValueMut<'a> for Frame<'a, $ack, $data>
        where ack::Ack<$ack>: DecoderParameterizedValueMut<'a, Parameter = Tag> {
            #[inline]
            fn decode_mut(buffer: DecoderBufferMut<'a>) -> DecoderBufferMutResult<'a, Self> {
                BasicFrameDecoder.decode_frame(buffer)
            }
        }

        impl<'a, $ack: ack::AckRanges, $data: EncoderValue> EncoderValue for Frame<'a, $ack, $data> {
            #[inline]
            fn encode<E: Encoder>(&self, buffer: &mut E)  {
                match self {
                    $(
                        Frame::$ty(frame) => buffer.encode(frame),
                    )*
                }
            }
        }

        struct BasicFrameDecoder;

        impl<'a, $ack, $data: DecoderValueMut<'a>> FrameDecoder<'a, $ack, $data> for BasicFrameDecoder
        where ack::Ack<$ack>: DecoderParameterizedValueMut<'a, Parameter = Tag> {
            type Output = Frame<'a, $ack, $data>;

            $(
                #[inline]
                fn $handler(&mut self, frame: $module::$ty $(<$($generics)*>)?) -> Result<Self::Output, DecoderError> {
                    Ok(Frame::$ty(frame))
                }
            )*
        }

        pub trait FrameDecoder<'a, $ack, $data: DecoderValueMut<'a>>
        where ack::Ack<$ack>: DecoderParameterizedValueMut<'a, Parameter = Tag> {
            type Output;

            $(
                fn $handler(&mut self, frame: $module::$ty $(<$($generics)*>)?) -> Result<Self::Output, DecoderError>;
            )*

            #[inline]
            fn handle_extension_frame(&mut self, buffer: DecoderBufferMut<'a>) -> DecoderBufferMutResult<'a, Self::Output> {
                let _ = buffer;

                Err(DecoderError::InvariantViolation("invalid frame"))
            }

            #[inline]
            fn decode_frame(
                &mut self,
                buffer: DecoderBufferMut<'a>,
            ) -> DecoderBufferMutResult<'a, Self::Output> {
                let tag = buffer.peek_byte(0)?;
                match tag {
                    // Make sure the single byte frame tags fit into a small variable-integer
                    // otherwise fallback to extension selection
                    0b0100_0000..=0xff => self.handle_extension_frame(buffer),
                    $(
                        $tag_macro!() => {
                            let buffer = buffer.skip(core::mem::size_of::<Tag>())?;
                            let (frame, buffer) = buffer.decode_parameterized(tag)?;
                            let output = self.$handler(frame)?;
                            Ok((output, buffer))
                        },
                    )*
                    _ => self.handle_extension_frame(buffer),
                }
            }
        }

        #[cfg(test)]
        mod snapshots {
            use super::*;
            use s2n_codec::assert_codec_round_trip_sample_file;

            $(
                #[test]
                fn $module() {
                    assert_codec_round_trip_sample_file!(FrameMut, concat!(
                        "src/frame/test_samples/",
                        stringify!($module),
                        ".bin"
                    ));
                }
            )*
        }
    };
}

// This implements a codec for a frame that contains simple
// values that don't vary based on the tag
macro_rules! simple_frame_codec {
    ($name:ident {
        $(
            $field:ident
        ),*
    }, $tag:expr) => {
        s2n_codec::decoder_parameterized_value!(
            impl<'a> $name {
                fn decode(_tag: crate::frame::Tag, buffer: Buffer) -> Result<Self> {
                    $(
                        let ($field, buffer) = buffer.decode()?;
                    )*

                    let frame = $name { $($field),* };

                    Ok((frame, buffer))
                }
            }
        );

        impl s2n_codec::EncoderValue for $name {
            #[inline]
            fn encode<E: s2n_codec::Encoder>(&self, buffer: &mut E) {
                buffer.encode(&$tag);
                $(
                    buffer.encode(&self.$field);
                )*
            }
        }
    };
}

frames! {
    AckRanges, Data |
    padding_tag => padding, handle_padding_frame, Padding;
    ping_tag => ping, handle_ping_frame, Ping;
    ack_tag => ack, handle_ack_frame, Ack[AckRanges];
    reset_stream_tag => reset_stream, handle_reset_stream_frame, ResetStream;
    stop_sending_tag => stop_sending, handle_stop_sending_frame, StopSending;
    crypto_tag => crypto, handle_crypto_frame, Crypto[Data];
    new_token_tag => new_token, handle_new_token_frame, NewToken['a];
    stream_tag => stream, handle_stream_frame, Stream[Data];
    max_data_tag => max_data, handle_max_data_frame, MaxData;
    max_stream_data_tag => max_stream_data, handle_max_stream_data_frame, MaxStreamData;
    max_streams_tag => max_streams, handle_max_streams_frame, MaxStreams;
    data_blocked_tag => data_blocked, handle_data_blocked_frame, DataBlocked;
    stream_data_blocked_tag => stream_data_blocked, handle_stream_data_blocked_frame, StreamDataBlocked;
    streams_blocked_tag => streams_blocked, handle_streams_blocked_frame, StreamsBlocked;
    new_connection_id_tag => new_connection_id, handle_new_connection_id_frame, NewConnectionId['a];
    retire_connection_id_tag => retire_connection_id, handle_retire_connection_id_frame, RetireConnectionId;
    path_challenge_tag => path_challenge, handle_path_challenge_frame, PathChallenge['a];
    path_response_tag => path_response, handle_path_response_frame, PathResponse['a];
    connection_close_tag => connection_close, handle_connection_close_frame, ConnectionClose['a];
    handshake_done_tag => handshake_done, handle_handshake_done_frame, HandshakeDone;
    datagram_tag => datagram, handle_datagram_frame, Datagram[Data];
}

#[derive(Clone, Copy, Debug, Default)]
/// Indicates the packet will not fit into the provided capacity
pub struct FitError;

impl fmt::Display for FitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "the frame could not fit into the provided capacity")
    }
}
