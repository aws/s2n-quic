// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::EncoderValue;
use s2n_quic_core::frame::{self, FitError};

#[derive(Clone, Copy, Debug, Default)]
pub struct Stream;

impl FrameWriter for Stream {
    type Context = VarInt;

    fn write_chunk<W: WriteContext>(
        &self,
        offset: VarInt,
        data: &mut View,
        stream_id: Self::Context,
        context: &mut W,
    ) -> Result<(), FitError> {
        let remaining_capacity = context.remaining_capacity();

        debug_assert!(
            data.len() <= remaining_capacity,
            "the data sender should not pass a payload that exceeds the current capacity"
        );

        let mut frame = frame::Stream {
            stream_id,
            offset,
            // this will be updated by `try_fit`
            is_last_frame: false,
            // this will be updated after we've made sure the frame fits
            is_fin: false,
            data,
        };

        let len = frame.try_fit(remaining_capacity)?;
        if len == 0 {
            return Err(FitError);
        }

        frame.data.trim_off(frame.data.encoding_size() - len)?;
        frame.is_fin = frame.data.is_fin();

        context.write_frame(&frame).ok_or(FitError)?;

        Ok(())
    }

    fn write_fin<W: WriteContext>(
        &self,
        offset: VarInt,
        stream_id: Self::Context,
        context: &mut W,
    ) -> Result<(), FitError> {
        let mut frame = frame::Stream {
            stream_id,
            offset,
            is_last_frame: false,
            is_fin: true,
            data: &[][..],
        };

        // the length is always 0 so we don't need to trim the data
        frame.try_fit(context.remaining_capacity())?;
        context.write_frame(&frame).ok_or(FitError)?;

        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Crypto;

impl FrameWriter for Crypto {
    type Context = ();

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.6
    //# The stream does not have an explicit end, so CRYPTO frames do not
    //# have a FIN bit.
    const WRITES_FIN: bool = false;

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.4
    //# When the PTO timer expires, an ack-eliciting packet MUST be sent.  An
    //# endpoint SHOULD include new data in this packet.  Previously sent
    //# data MAY be sent if no new data can be sent.
    // Allow already transmitted, unacked crypto frames to be included in
    // probe packets in anticipation the crypto frames were lost. This will
    // help the handshake recover from packet loss.
    const RETRANSMIT_IN_PROBE: bool = true;

    fn write_chunk<W: WriteContext>(
        &self,
        offset: VarInt,
        data: &mut View,
        _writer_context: Self::Context,
        context: &mut W,
    ) -> Result<(), FitError> {
        let remaining_capacity = context.remaining_capacity();
        debug_assert!(
            data.len() <= remaining_capacity,
            "the data sender should not pass a payload that exceeds the current capacity"
        );

        // Some QUIC implementations refuse to process empty CRYPTO frames so
        // make sure we never send them
        debug_assert_ne!(data.len(), 0u64);

        let frame = frame::Crypto { offset, data };

        let len = frame.try_fit(remaining_capacity)?;
        if len == 0 {
            return Err(FitError);
        }

        frame.data.trim_off(frame.data.encoding_size() - len)?;

        context.write_frame(&frame).ok_or(FitError)?;

        Ok(())
    }

    fn write_fin<W: WriteContext>(
        &self,
        _offset: VarInt,
        _writer_context: Self::Context,
        _context: &mut W,
    ) -> Result<(), FitError> {
        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.6
        //# The stream does not have an explicit end, so CRYPTO frames do not
        //# have a FIN bit.
        // do nothing
        Ok(())
    }
}
