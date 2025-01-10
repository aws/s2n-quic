// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::Credentials,
    crypto::seal,
    packet::stream::{self, encoder},
    stream::{
        packet_number,
        send::{application::transmission, error::Error, flow, path, probes},
    },
};
use bytes::buf::UninitSlice;
use s2n_codec::EncoderBuffer;
use s2n_quic_core::{
    buffer::{self, reader::Storage as _, Reader as _},
    ensure,
    time::Clock,
    varint::VarInt,
};

pub trait Message {
    fn max_segments(&self) -> usize;
    fn push<P: FnOnce(&mut UninitSlice) -> transmission::Event<()>>(
        &mut self,
        buffer_len: usize,
        p: P,
    );
}

#[derive(Clone, Copy, Debug)]
pub struct State {
    pub stream_id: stream::Id,
    pub source_control_port: u16,
    pub source_stream_port: Option<u16>,
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
        credentials: &Credentials,
        clock: &Clk,
        message: &mut M,
    ) -> Result<(), Error>
    where
        E: seal::Application,
        I: buffer::reader::Storage<Error = core::convert::Infallible>,
        Clk: Clock,
        M: Message,
    {
        ensure!(
            credits.len > 0 || storage.buffer_is_empty() || credits.is_fin,
            Ok(())
        );

        let mut reader = buffer::reader::Incremental::new(credits.offset);
        let mut reader = reader.with_storage(storage, credits.is_fin)?;
        debug_assert!(
            reader.buffered_len() >= credits.len,
            "attempted to acquire more credits than what is buffered"
        );
        let mut reader = reader.with_read_limit(credits.len);

        let stream_id = *self.stream_id();
        let max_header_len = self.max_header_len();

        let mut total_payload_len = 0;

        loop {
            let packet_number = packet_number.next()?;

            let buffer_len = {
                let estimated_len = reader.buffered_len() + max_header_len;
                (path.max_datagram_size as usize).min(estimated_len)
            };

            message.push(buffer_len, |buffer| {
                let stream_offset = reader.current_offset();
                let mut reader = reader.track_read();

                let buffer = unsafe {
                    // SAFETY: `buffer` is a valid `UninitSlice` but `EncoderBuffer` expects to
                    // write into a `&mut [u8]`. Here we construct a `&mut [u8]` since
                    // `EncoderBuffer` never actually reads from the slice and only writes to it.
                    core::slice::from_raw_parts_mut(buffer.as_mut_ptr(), buffer.len())
                };
                let encoder = EncoderBuffer::new(buffer);
                let packet_len = encoder::encode(
                    encoder,
                    self.source_control_port,
                    self.source_stream_port,
                    stream_id,
                    packet_number,
                    path.next_expected_control_packet,
                    VarInt::ZERO,
                    &mut &[][..],
                    VarInt::ZERO,
                    &(),
                    &mut reader,
                    encrypt_key,
                    credentials,
                );

                // buffer is clamped to u16::MAX so this is safe to cast without loss
                let _: u16 = path.max_datagram_size;
                let packet_len = packet_len as u16;
                let payload_len = reader.consumed_len() as u16;
                total_payload_len += payload_len as usize;

                let has_more_app_data = credits.initial_len > total_payload_len;

                let included_fin = reader.final_offset().is_some_and(|fin| {
                    stream_offset.as_u64() + payload_len as u64 == fin.as_u64()
                });

                let time_sent = clock.get_time();
                probes::on_transmit_stream(
                    credentials.id,
                    stream_id,
                    stream::PacketSpace::Stream,
                    s2n_quic_core::packet::number::PacketNumberSpace::Initial
                        .new_packet_number(packet_number),
                    stream_offset,
                    payload_len,
                    included_fin,
                    false,
                );

                let info = transmission::Info {
                    packet_len,
                    retransmission: if stream_id.is_reliable {
                        Some(())
                    } else {
                        None
                    },
                    stream_offset,
                    payload_len,
                    included_fin,
                    time_sent,
                    ecn: path.ecn,
                };

                transmission::Event {
                    packet_number,
                    info,
                    has_more_app_data,
                }
            });

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
