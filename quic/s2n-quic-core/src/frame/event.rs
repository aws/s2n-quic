// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_codec::EncoderValue;

use crate::event::common;

/// Trait to retrieve subcription event type of the frame
pub trait AsEvent {
    fn as_event(&self) -> common::Frame;
}

macro_rules! as_event {
    ($(
        $module:ident, $ty:ident $([$($generics:tt)+])?;
     )*) => {
        $(
            use super::$module;

            impl$(<$($generics)*>)? AsEvent for $module::$ty $(<$($generics)*>)? {
                #[inline]
                fn as_event(&self) -> common::Frame {
                    common::Frame::$ty
                }
            }
        )*
    }
}

as_event! {
    padding, Padding;
    ping, Ping;
    ack, Ack[AckRanges];
    reset_stream, ResetStream;
    stop_sending, StopSending;
    new_token, NewToken['a];
    max_data, MaxData;
    max_stream_data, MaxStreamData;
    max_streams, MaxStreams;
    data_blocked, DataBlocked;
    stream_data_blocked, StreamDataBlocked;
    streams_blocked, StreamsBlocked;
    new_connection_id, NewConnectionId['a];
    retire_connection_id, RetireConnectionId;
    path_challenge, PathChallenge['a];
    path_response, PathResponse['a];
    connection_close, ConnectionClose['a];
    handshake_done, HandshakeDone;
}

impl<Data> AsEvent for super::stream::Stream<Data>
where
    Data: EncoderValue,
{
    #[inline]
    fn as_event(&self) -> common::Frame {
        common::Frame::Stream {
            id: self.stream_id.as_u64(),
            offset: self.offset.as_u64(),
            len: self.data.encoding_size() as _,
            is_fin: self.is_fin,
        }
    }
}

impl<Data> AsEvent for super::crypto::Crypto<Data>
where
    Data: EncoderValue,
{
    #[inline]
    fn as_event(&self) -> common::Frame {
        common::Frame::Crypto {
            offset: self.offset.as_u64(),
            len: self.data.encoding_size() as _,
        }
    }
}
