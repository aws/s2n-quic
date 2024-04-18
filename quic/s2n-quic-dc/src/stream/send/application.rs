// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    crypto::encrypt,
    packet::stream::{self, encoder},
    stream::{
        packet_number,
        send::{error::Error, flow, path},
    },
};
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    buffer::{self, reader::Storage as _, Reader as _},
    ensure,
    inet::ExplicitCongestionNotification,
    time::Clock,
    varint::VarInt,
};

pub trait Message {
    fn max_segments(&self) -> usize;
    fn set_ecn(&mut self, ecn: ExplicitCongestionNotification);
    fn push<Clk: Clock, P: FnOnce(&mut [u8]) -> Result<usize, E>, E>(
        &mut self,
        clock: &Clk,
        is_reliable: bool,
        buffer_len: usize,
        p: P,
    ) -> Result<(), E>;
}

pub struct State {
    pub source_control_port: u16,
    pub stream_id: stream::Id,
}

impl State {
    #[inline]
    pub fn transmit<E, I, Clk, M>(
        &self,
        credits: flow::Credits,
        path: &path::Info,
        storage: &mut I,
        packet_number: &packet_number::Counter,
        encrypt_key: &E,
        clock: &Clk,
        message: &mut M,
    ) -> Result<(), Error>
    where
        E: encrypt::Key,
        I: buffer::reader::Storage<Error = core::convert::Infallible>,
        Clk: Clock,
        M: Message,
    {
        ensure!(credits.len > 0 || credits.is_fin, Ok(()));

        let mut reader = buffer::reader::Incremental::new(credits.offset);
        let mut reader = reader.with_storage(storage, credits.is_fin)?;
        debug_assert!(
            reader.buffered_len() >= credits.len,
            "attempted to acquire more credits than what is buffered"
        );
        let mut reader = reader.with_read_limit(credits.len);

        let stream_id = *self.stream_id();
        let max_header_len = self.max_header_len();

        // TODO set destination address with the current value

        message.set_ecn(path.ecn);

        loop {
            let packet_number = packet_number.next()?;

            let buffer_len = {
                let estimated_len = reader.buffered_len() + max_header_len;
                (path.mtu as usize).min(estimated_len)
            };

            message.push(clock, stream_id.is_reliable, buffer_len, |buffer| {
                let encoder = EncoderBuffer::new(buffer);
                encoder::encode(
                    encoder,
                    self.source_control_port,
                    None,
                    stream_id,
                    packet_number,
                    path.next_expected_control_packet,
                    VarInt::ZERO,
                    &mut &[][..],
                    VarInt::ZERO,
                    &(),
                    &mut reader,
                    encrypt_key,
                )
            })?;

            // bail if we've transmitted everything
            ensure!(!reader.buffer_is_empty(), break);
        }

        Ok(())
    }

    #[inline]
    fn stream_id(&self) -> &stream::Id {
        &self.stream_id
    }

    #[inline]
    pub fn max_header_len(&self) -> usize {
        if self.stream_id().is_reliable {
            encoder::MAX_RETRANSMISSION_HEADER_LEN
        } else {
            encoder::MAX_HEADER_LEN
        }
    }
}
